use std::cmp::max;
use std::iter::zip;
use std::rc::Rc;
use std::time::Duration;

use nyx_config::utils::MergeWith as _;
use nyx_config::{PresetSize, Struts};
use nyx_ipc::{PositionChange, SizeChange, WindowLayout};
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::utils::{Logical, Point, Rectangle, Scale, Serial, Size};

use super::closing_window::{ClosingWindow, ClosingWindowRenderElement};
use super::tile::{Tile, TileRenderElement, TileRenderSnapshot};
use super::workspace::{InteractiveResize, ResolvedSize};
use super::{ConfigureIntent, HitType, InteractiveResizeData, LayoutElement, Options, RemovedTile};
use crate::animation::{Animation, Clock};
use crate::canvas::spring::SpringParams as CanvasSpringParams;
use crate::canvas::tiling::{DynamicTiling, TilingKind, TilingRegion};
use crate::canvas::viewport::Viewport;
use crate::layout::{RenderLayer, SizingMode};
use crate::niri_render_elements;
use crate::render_helpers::renderer::NiriRenderer;
use crate::render_helpers::xray::XrayPos;
use crate::render_helpers::RenderCtx;
use crate::utils::transaction::{Transaction, TransactionBlocker};
use crate::utils::{ensure_min_max_size, ensure_min_max_size_maybe_zero, ResizeEdge};
use crate::window::ResolvedWindowRules;

/// Keyboard nudge step for `move_*` actions, in canvas units.
pub const CANVAS_NUDGE_PX: f64 = 50.;

/// Free 2D canvas holding one tile per window at absolute canvas coordinates.
#[derive(Debug)]
pub struct CanvasSpace<W: LayoutElement> {
    /// Tiles in top-to-bottom (front-to-back) order.
    tiles: Vec<Tile<W>>,

    /// Extra per-tile data, parallel to `tiles`.
    data: Vec<CanvasData>,

    active_window_id: Option<W::Id>,

    interactive_resize: Option<InteractiveResize<W>>,

    closing_windows: Vec<ClosingWindow>,

    /// View size for this space (output logical size).
    view_size: Size<f64, Logical>,

    /// Working area (layer-shell exclusive zones + struts taken into account).
    working_area: Rectangle<f64, Logical>,

    /// Working area excluding struts, for popup unconstraining.
    parent_area: Rectangle<f64, Logical>,

    scale: f64,

    clock: Clock,

    last_camera_advance: Option<Duration>,

    options: Rc<Options>,

    canvas_viewport: Viewport,

    /// Freeze camera springs while pointer buttons are held (no grab).
    ///
    /// Set together with the layout-level press deferral: the world must not
    /// move mid-gesture, or press, motion and release map through different
    /// transforms. Unfrozen on release and whenever a grab takes over.
    camera_frozen: bool,

    /// Pre-fullscreen camera targets, saved on entering exclusive fullscreen
    /// and restored on exit so navigation resumes exactly where it was.
    saved_view: Option<(f64, f64, f64)>,

    /// Optional dynamic tiling policy applied on top of the free canvas.
    canvas_tiling: DynamicTiling,

    /// Manual tiling toggle (`toggle-dynamic-tiling`).
    ///
    /// `None` means "follow the config"; `Some(kind)` overrides it until the next
    /// toggle, so a config reload doesn't silently undo what the user asked for.
    tiling_override: Option<TilingKind>,

    /// Slot order for dynamic tiling, by window id.
    ///
    /// Kept separately from `tiles` (which is the stacking order and must keep
    /// children above parents), so swapping two tiled windows can never break the
    /// stacking invariants. `tiling_order[0]` is the master slot.
    tiling_order: Vec<W::Id>,

    /// Canvas rect the tiling layout fills, anchored when tiling was switched on.
    ///
    /// Anchoring it means panning and zooming move the camera *over* the tiled
    /// cluster instead of dragging the cluster along with the view.
    tiling_region: Option<TilingRegion>,

    /// Camera zoom the tiling region was anchored at.
    tiling_zoom: f64,
}

niri_render_elements! {
    CanvasSpaceRenderElement<R> => {
        Tile = TileRenderElement<R>,
        ClosingWindow = ClosingWindowRenderElement,
    }
}

/// Extra per-tile data for the free canvas.
#[derive(Debug, Clone, Copy, PartialEq)]
struct CanvasData {
    /// Top-left corner of the tile in absolute canvas coordinates (R²).
    pos: Point<f64, Logical>,

    /// Cached tile size (including borders).
    size: Size<f64, Logical>,

    /// Currently selected width preset index, if any.
    preset_width_idx: Option<usize>,

    /// Currently selected height preset index, if any.
    preset_height_idx: Option<usize>,

    /// Tile width to restore when leaving the full-width state.
    full_width_restore: Option<f64>,

    /// Window size to restore when leaving fullscreen/maximized.
    unfs_restore: Option<Size<i32, Logical>>,

    /// Canvas position and window size from before dynamic tiling took over.
    ///
    /// Set when the tile is first laid out by the tiler, consumed when tiling is
    /// switched off: that's how windows fly back to where they were on the
    /// infinite canvas.
    untiled: Option<UntiledState>,
}

/// Free-canvas state remembered for a window while dynamic tiling holds it.
#[derive(Debug, Clone, Copy, PartialEq)]
struct UntiledState {
    /// Top-left corner in canvas coordinates.
    pos: Point<f64, Logical>,
    /// Window size (not tile size: borders are re-added on restore).
    win_size: Size<i32, Logical>,
}

impl CanvasData {
    fn new(tile: &Tile<impl LayoutElement>, pos: Point<f64, Logical>) -> Self {
        let mut rv = Self {
            pos,
            size: Size::default(),
            preset_width_idx: None,
            preset_height_idx: None,
            full_width_restore: None,
            unfs_restore: None,
            untiled: None,
        };
        rv.update(tile);
        rv
    }

    fn update(&mut self, tile: &Tile<impl LayoutElement>) {
        self.size = tile.tile_size();
    }

    /// Center of the tile in canvas coordinates.
    fn center(&self) -> Point<f64, Logical> {
        self.pos + self.size.to_point().downscale(2.)
    }

    fn rect(&self) -> Rectangle<f64, Logical> {
        Rectangle::new(self.pos, self.size)
    }
}

impl<W: LayoutElement> CanvasSpace<W> {
    pub fn new(
        view_size: Size<f64, Logical>,
        parent_area: Rectangle<f64, Logical>,
        scale: f64,
        clock: Clock,
        options: Rc<Options>,
    ) -> Self {
        let working_area = compute_working_area(parent_area, scale, options.layout.struts);

        let spring = CanvasSpringParams {
            stiffness: options.canvas.spring.stiffness,
            damping_ratio: options.canvas.spring.damping_ratio,
            mass: options.canvas.spring.mass,
            epsilon: 0.001,
        };
        let mut viewport = Viewport::new(view_size.w.max(1.0), view_size.h.max(1.0), spring);
        viewport.configure_camera(&options.canvas);

        let tiling = canvas_tiling_from_options(&options);

        Self {
            tiles: Vec::new(),
            data: Vec::new(),
            active_window_id: None,
            interactive_resize: None,
            closing_windows: Vec::new(),
            view_size,
            working_area,
            parent_area,
            scale,
            clock,
            last_camera_advance: None,
            options,
            canvas_viewport: viewport,
            canvas_tiling: tiling,
            tiling_override: None,
            tiling_order: Vec::new(),
            tiling_region: None,
            tiling_zoom: 1.,
            saved_view: None,
            camera_frozen: false,
        }
    }

    /// Freeze or unfreeze the camera springs (see field docs).
    pub fn set_camera_frozen(&mut self, frozen: bool) {
        self.camera_frozen = frozen;
    }

    pub fn update_config(
        &mut self,
        view_size: Size<f64, Logical>,
        parent_area: Rectangle<f64, Logical>,
        scale: f64,
        options: Rc<Options>,
    ) {
        let working_area = compute_working_area(parent_area, scale, options.layout.struts);

        // Re-anchoring the tiling region is only needed when the geometry we
        // anchored it to actually moved.
        let geometry_changed = self.view_size != view_size || self.working_area != working_area;

        for (tile, data) in zip(&mut self.tiles, &mut self.data) {
            tile.update_config(view_size, scale, options.clone());
            data.update(tile);
        }

        self.view_size = view_size;
        self.working_area = working_area;
        self.parent_area = parent_area;
        self.scale = scale;
        self.options = options;

        self.canvas_viewport
            .set_output_size(view_size.w.max(1.0), view_size.h.max(1.0));
        self.canvas_viewport
            .configure_camera(&self.options.canvas);
        self.canvas_viewport.set_spring_params(CanvasSpringParams {
            stiffness: self.options.canvas.spring.stiffness,
            damping_ratio: self.options.canvas.spring.damping_ratio,
            mass: self.options.canvas.spring.mass,
            epsilon: 0.001,
        });
        let prev = self.canvas_tiling;
        let from_config = canvas_tiling_from_options(&self.options);
        let kind = self.tiling_override.unwrap_or(from_config.kind);
        self.canvas_tiling = DynamicTiling::new(kind, from_config.gap, from_config.master_ratio);

        let policy_changed = prev.kind != self.canvas_tiling.kind
            || prev.gap != self.canvas_tiling.gap
            || prev.master_ratio != self.canvas_tiling.master_ratio;

        if self.canvas_tiling.is_on() {
            if self.tiling_region.is_none() || geometry_changed {
                self.anchor_tiling_region();
            }
            if policy_changed || geometry_changed {
                self.sync_tiling_order();
                self.retile();
            }
        } else if prev.is_on() {
            self.untile_all();
        }
    }

    pub fn update_shaders(&mut self) {
        for tile in &mut self.tiles {
            tile.update_shaders();
        }
    }

    pub fn advance_animations(&mut self) {
        // Instantly-complete mode (tests, loading screens) snaps the camera.
        if self.clock.should_complete_instantly() {
            self.canvas_viewport.snap_to_targets();
        }

        // Advance the camera springs with the frame-clock delta.
        let now = self.clock.now();
        let dt = match self.last_camera_advance {
            Some(last) => now.saturating_sub(last),
            None => Duration::ZERO,
        };
        self.last_camera_advance = Some(now);
        // Clamp huge deltas (e.g. after sleep) so the camera doesn't jump.
        let dt = dt.min(Duration::from_millis(100));
        // Frozen while pointer buttons are held: hold live values so the
        // gesture maps through one transform. Timestamp still advances for a
        // smooth resume; tiles keep animating normally.
        if !self.camera_frozen {
            self.canvas_viewport.advance(dt);
        }

        for tile in &mut self.tiles {
            tile.advance_animations();
        }

        self.closing_windows.retain_mut(|closing| {
            closing.advance_animations();
            closing.are_animations_ongoing()
        });
    }

    pub fn are_animations_ongoing(&self) -> bool {
        self.canvas_viewport.is_animating()
            || self.tiles.iter().any(Tile::are_animations_ongoing)
            || !self.closing_windows.is_empty()
    }

    pub fn are_transitions_ongoing(&self) -> bool {
        self.canvas_viewport.is_animating()
            || self.tiles.iter().any(Tile::are_transitions_ongoing)
            || !self.closing_windows.is_empty()
    }

    pub fn update_render_elements(&mut self, is_active: bool, layer: RenderLayer) {
        // Cull against the visible canvas rect.
        let (ox, oy) = self.canvas_viewport.visible_origin();
        let (vw, vh) = self.canvas_viewport.visible_size();
        let view_rect = Rectangle::new(Point::from((ox, oy)), Size::from((vw, vh)));

        let active = self.active_window_id.clone();
        for (tile, data) in zip(&mut self.tiles, &self.data) {
            // Skip tiles belonging to a different render layer.
            if layer.is_normal() == tile.is_moving_between_workspaces() {
                continue;
            }

            let id = tile.window().id();
            let is_active = is_active && Some(id) == active.as_ref();

            // The exclusive fullscreen tile renders in screen space at exact
            // output size: give it a tile-local full-output rect instead of
            // the camera-relative one so its elements never go stale.
            let mut tile_view_rect = view_rect;
            if tile.window().sizing_mode().is_fullscreen() && Some(id) == active.as_ref() {
                tile_view_rect = Rectangle::from_size(self.view_size);
            } else {
                tile_view_rect.loc -= data.pos + tile.render_offset();
            }
            tile.update_render_elements(is_active, tile_view_rect);
        }
    }

    pub fn tiles(&self) -> impl Iterator<Item = &Tile<W>> + '_ {
        self.tiles.iter()
    }

    pub fn tiles_mut(&mut self) -> impl Iterator<Item = &mut Tile<W>> + '_ {
        self.tiles.iter_mut()
    }

    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }

    pub fn has_window(&self, id: &W::Id) -> bool {
        self.idx_of(id).is_some()
    }

    fn idx_of(&self, id: &W::Id) -> Option<usize> {
        self.tiles.iter().position(|tile| tile.window().id() == id)
    }

    pub fn active_window(&self) -> Option<&W> {
        let id = self.active_window_id.as_ref()?;
        self.tiles
            .iter()
            .find(|tile| tile.window().id() == id)
            .map(Tile::window)
    }

    pub fn active_window_mut(&mut self) -> Option<&mut W> {
        let id = self.active_window_id.as_ref()?;
        self.tiles
            .iter_mut()
            .find(|tile| tile.window().id() == id)
            .map(Tile::window_mut)
    }

    pub fn active_tile_mut(&mut self) -> Option<&mut Tile<W>> {
        let id = self.active_window_id.clone()?;
        self.tiles.iter_mut().find(|tile| tile.window().id() == &id)
    }

    pub fn active_id(&self) -> Option<W::Id>
    where
        W::Id: Clone,
    {
        self.active_window_id.clone()
    }

    pub fn is_active_pending_fullscreen(&self) -> bool {
        let Some(id) = &self.active_window_id else {
            return false;
        };
        self.tiles
            .iter()
            .find(|tile| tile.window().id() == id)
            .is_some_and(|tile| tile.window().pending_sizing_mode().is_fullscreen())
    }

    pub fn new_window_toplevel_bounds(&self, rules: &ResolvedWindowRules) -> Size<i32, Logical> {
        let border_config = self.options.layout.border.merged_with(&rules.border);
        compute_toplevel_bounds(border_config, self.working_area.size)
    }

    pub fn new_window_size(
        &self,
        width: Option<PresetSize>,
        height: Option<PresetSize>,
        rules: &ResolvedWindowRules,
    ) -> Size<i32, Logical> {
        let border = self.options.layout.border.merged_with(&rules.border);

        let resolve = |size: Option<PresetSize>, working_area_size: f64| {
            if let Some(size) = size {
                let size = match resolve_preset_size(size, working_area_size) {
                    ResolvedSize::Tile(mut size) => {
                        if !border.off {
                            size -= border.width * 2.;
                        }
                        size
                    }
                    ResolvedSize::Window(size) => size,
                };

                max(1, size.floor() as i32)
            } else {
                0
            }
        };

        let width = resolve(width, self.working_area.size.w);
        let height = resolve(height, self.working_area.size.h);

        Size::from((width, height))
    }

    /// Canvas position for a new window: center of the current camera view.
    ///
    /// Like vxwm's `manage()`, which centers new clients in the visible area.
    /// This stays the *free* position even when dynamic tiling is on: the tiler
    /// moves the window into its slot right after [`add_tile`](Self::add_tile),
    /// and remembers this position so the window can fly back here when tiling
    /// is switched off.
    pub fn spawn_position(&self, tile: &Tile<W>) -> Point<f64, Logical> {
        let size = tile.tile_size();
        let (x, y) = self.canvas_viewport.spawn_center(size.w, size.h);
        Point::from((x, y))
    }

    /// No automatic arrangement takes place: the tile stays exactly where put.
    pub fn add_tile(
        &mut self,
        mut tile: Tile<W>,
        pos: Point<f64, Logical>,
        activate: bool,
        _anim: Option<nyx_config::Animation>,
    ) {
        tile.update_config(self.view_size, self.scale, self.options.clone());

        // Honor a pending maximized/fullscreen state, sizing the tile accordingly.
        // Otherwise the tile keeps whatever size it has (fresh windows honor
        // their initial configure; moved windows keep their committed size),
        // so no size request is issued here.
        match tile.window().pending_sizing_mode() {
            SizingMode::Normal => (),
            SizingMode::Maximized => {
                tile.request_maximized(self.working_area.size, false, None);
            }
            SizingMode::Fullscreen => {
                tile.request_fullscreen(false, None);
            }
        }

        if activate || self.tiles.is_empty() {
            self.active_window_id = Some(tile.window().id().clone());
        }

        // Make sure the tile isn't inserted below its parent.
        // New windows go on top; descendants are raised above right after.
        let idx = 0;
        let new_id = tile.window().id().clone();
        let data = CanvasData::new(&tile, pos);
        self.data.insert(idx, data);
        self.tiles.insert(idx, tile);

        self.bring_up_descendants_of(idx);

        // New windows take the master slot, dwm/Hyprland style.
        if !self.tiling_order.iter().any(|id| id == &new_id) {
            self.tiling_order.insert(0, new_id.clone());
        }

        if self.canvas_tiling.is_on() {
            self.retile();

            // A window born into the tiling layout has no earlier free-canvas
            // home, and the spawn center is the same point for all of them.
            // Hand it the slot it just landed in instead, so switching tiling
            // off doesn't pile every window up in the middle of the view.
            if let Some(new_idx) = self.idx_of(&new_id) {
                let pos = self.data[new_idx].pos;
                let win = self.tiles[new_idx].window();
                let size = win.expected_size().unwrap_or_else(|| win.size());
                self.data[new_idx].untiled = Some(UntiledState {
                    pos,
                    win_size: Size::from((size.w.max(1), size.h.max(1))),
                });
            }
        }

        if activate {
            let id = self.tiles[idx].window().id().clone();
            self.center_on_window(&id);
        }
    }

    fn bring_up_descendants_of(&mut self, idx: usize) {
        let tile = &self.tiles[idx];
        let win = tile.window();

        // We always maintain the correct stacking order, so walking descendants back to front
        // should give us all of them.
        let mut descendants: Vec<usize> = Vec::new();
        for (i, tile_below) in self.tiles.iter().enumerate().skip(idx + 1).rev() {
            let win_below = tile_below.window();
            if win_below.is_child_of(win)
                || descendants
                    .iter()
                    .any(|idx| win_below.is_child_of(self.tiles[*idx].window()))
            {
                descendants.push(i);
            }
        }

        // Now, descendants is in back-to-front order, and repositioning them in the front-to-back
        // order will preserve the subsequent indices and work out right.
        let mut idx = idx;
        #[allow(clippy::explicit_counter_loop)]
        for descendant_idx in descendants.into_iter().rev() {
            self.raise_window(descendant_idx, idx);
            idx += 1;
        }
    }

    fn raise_window(&mut self, from_idx: usize, to_idx: usize) {
        assert!(to_idx <= from_idx);

        let tile = self.tiles.remove(from_idx);
        let data = self.data.remove(from_idx);
        self.tiles.insert(to_idx, tile);
        self.data.insert(to_idx, data);
    }

    pub fn remove_tile(&mut self, window: &W::Id, _transaction: Transaction) -> RemovedTile<W> {
        let idx = self.idx_of(window).unwrap();
        self.remove_tile_by_idx(idx)
    }

    fn remove_tile_by_idx(&mut self, idx: usize) -> RemovedTile<W> {
        let tile = self.tiles.remove(idx);
        let data = self.data.remove(idx);

        {
            let removed_id = tile.window().id();
            self.tiling_order.retain(|id| id != removed_id);
        }

        if self.tiles.is_empty() {
            self.active_window_id = None;
        } else if Some(tile.window().id()) == self.active_window_id.as_ref() {
            // The active tile was removed; activate the topmost tile.
            self.active_window_id = Some(self.tiles[0].window().id().clone());
        }

        // Stop interactive resize.
        if let Some(resize) = &self.interactive_resize {
            if tile.window().id() == &resize.window {
                self.interactive_resize = None;
            }
        }

        // The remaining windows reflow into the freed space.
        if self.canvas_tiling.is_on() {
            self.retile();
        }

        // Hand back the free-canvas position when the tiler was holding the
        // window, so it lands sanely wherever it is re-inserted.
        let pos = data.untiled.map_or(data.pos, |untiled| untiled.pos);

        RemovedTile {
            tile,
            pos,
            is_floating: false,
        }
    }

    pub fn update_window(&mut self, window: &W::Id, serial: Option<Serial>) {
        let Some(tile_idx) = self.idx_of(window) else {
            return;
        };

        let tile = &mut self.tiles[tile_idx];
        let data = &mut self.data[tile_idx];

        let resize = tile.window_mut().interactive_resize_data();

        // Do this before calling update_window() so it can get up-to-date info.
        if let Some(serial) = serial {
            tile.window_mut().on_commit(serial);
        }

        let prev_size = data.size;

        tile.update_window();
        data.update(tile);

        // When resizing by top/left edge, update the position accordingly.
        if let Some(resize) = resize {
            let mut offset = Point::from((0., 0.));
            if resize.edges.contains(ResizeEdge::LEFT) {
                offset.x += prev_size.w - data.size.w;
            }
            if resize.edges.contains(ResizeEdge::TOP) {
                offset.y += prev_size.h - data.size.h;
            }
            data.pos += offset;
        }
    }

    /// Activates a window and centers the camera on it (focus-tracking).
    pub fn activate_window(&mut self, window: &W::Id) -> bool {
        if !self.has_window(window) {
            return false;
        }

        self.active_window_id = Some(window.clone());
        self.center_on_window(window);
        true
    }

    /// Activates a window without moving the camera (e.g. focus-follows-mouse).
    pub fn activate_window_without_raising(&mut self, window: &W::Id) -> bool {
        if !self.has_window(window) {
            return false;
        }

        self.active_window_id = Some(window.clone());
        true
    }

    pub fn start_close_animation_for_window(
        &mut self,
        renderer: &mut GlesRenderer,
        window: &W::Id,
        blocker: TransactionBlocker,
    ) {
        let (tile, tile_pos) = self
            .tiles_with_render_positions_mut(false)
            .find(|(tile, _)| tile.window().id() == window)
            .unwrap();

        let Some(snapshot) = tile.take_unmap_snapshot() else {
            return;
        };

        let tile_size = tile.tile_size();

        self.start_close_animation_for_tile(renderer, snapshot, tile_size, tile_pos, blocker);
    }

    pub fn start_close_animation_for_tile(
        &mut self,
        renderer: &mut GlesRenderer,
        snapshot: TileRenderSnapshot,
        tile_size: Size<f64, Logical>,
        tile_pos: Point<f64, Logical>,
        blocker: TransactionBlocker,
    ) {
        let style = &self.options.animations.window_close;
        let anim = Animation::new(
            self.clock.clone(),
            0.,
            1.,
            0.,
            style.anim,
        );

        let blocker = if self.options.disable_transactions {
            TransactionBlocker::completed()
        } else {
            blocker
        };

        let scale = Scale::from(self.scale);
        let res = ClosingWindow::new(
            renderer,
            snapshot,
            scale,
            tile_size,
            tile_pos,
            blocker,
            anim,
            style.popin_end,
            style.slide,
            style.fade,
        );
        match res {
            Ok(closing) => {
                self.closing_windows.push(closing);
            }
            Err(err) => {
                warn!("error creating a closing window animation: {err:?}");
            }
        }
    }

    pub fn start_open_animation(&mut self, id: &W::Id) -> bool {
        let Some(idx) = self.idx_of(id) else {
            return false;
        };

        self.tiles[idx].start_open_animation();
        true
    }

    fn focus_directional(
        &mut self,
        distance: impl Fn(Point<f64, Logical>, Point<f64, Logical>) -> f64,
    ) -> bool {
        let Some(active_id) = self.active_window_id.clone() else {
            return false;
        };
        let active_idx = self.idx_of(&active_id).unwrap();
        let center = self.data[active_idx].center();

        let result = zip(&self.tiles, &self.data)
            .filter(|(tile, _)| tile.window().id() != &active_id)
            .map(|(tile, data)| (tile.window().id().clone(), distance(center, data.center())))
            .filter(|(_, dist)| *dist > 0.)
            .min_by(|(_, dist_a), (_, dist_b)| f64::total_cmp(dist_a, dist_b));
        if let Some((id, _)) = result {
            self.activate_window(&id);
            true
        } else {
            false
        }
    }

    /// Spatial focus: nearest window center in the given direction.
    pub fn focus_left(&mut self) -> bool {
        self.focus_directional(|focus, other| focus.x - other.x)
    }

    pub fn focus_right(&mut self) -> bool {
        self.focus_directional(|focus, other| other.x - focus.x)
    }

    pub fn focus_up(&mut self) -> bool {
        self.focus_directional(|focus, other| focus.y - other.y)
    }

    pub fn focus_down(&mut self) -> bool {
        self.focus_directional(|focus, other| other.y - focus.y)
    }

    pub fn focus_leftmost(&mut self) {
        let result = self
            .tiles_with_canvas_positions()
            .min_by(|(_, pos_a), (_, pos_b)| f64::total_cmp(&pos_a.x, &pos_b.x));
        if let Some((tile, _)) = result {
            let id = tile.window().id().clone();
            self.activate_window(&id);
        }
    }

    pub fn focus_rightmost(&mut self) {
        let result = self
            .tiles_with_canvas_positions()
            .max_by(|(_, pos_a), (_, pos_b)| f64::total_cmp(&pos_a.x, &pos_b.x));
        if let Some((tile, _)) = result {
            let id = tile.window().id().clone();
            self.activate_window(&id);
        }
    }

    pub fn focus_topmost(&mut self) {
        let result = self
            .tiles_with_canvas_positions()
            .min_by(|(_, pos_a), (_, pos_b)| f64::total_cmp(&pos_a.y, &pos_b.y));
        if let Some((tile, _)) = result {
            let id = tile.window().id().clone();
            self.activate_window(&id);
        }
    }

    pub fn focus_bottommost(&mut self) {
        let result = self
            .tiles_with_canvas_positions()
            .max_by(|(_, pos_a), (_, pos_b)| f64::total_cmp(&pos_a.y, &pos_b.y));
        if let Some((tile, _)) = result {
            let id = tile.window().id().clone();
            self.activate_window(&id);
        }
    }

    /// Keyboard move: nudge the active tile across the canvas.
    fn nudge_active(&mut self, amount: Point<f64, Logical>) -> bool {
        let Some(active_id) = self.active_window_id.clone() else {
            return false;
        };
        let idx = self.idx_of(&active_id).unwrap();

        // While tiling is on, "moving" a window means swapping tiling slots,
        // like Hyprland's `movewindow`: free nudging would just be undone by
        // the next re-tile.
        if self.canvas_tiling.is_on() {
            let forwards = amount.x > 0. || amount.y > 0.;
            return self.move_tiling_slot(&active_id, forwards);
        }

        let new_pos = self.data[idx].pos + amount;
        self.move_to(idx, new_pos, true);
        self.interactive_resize_end(None);
        true
    }

    pub fn move_left(&mut self) -> bool {
        self.nudge_active(Point::from((-CANVAS_NUDGE_PX, 0.)))
    }

    pub fn move_right(&mut self) -> bool {
        self.nudge_active(Point::from((CANVAS_NUDGE_PX, 0.)))
    }

    pub fn move_up(&mut self) -> bool {
        self.nudge_active(Point::from((0., -CANVAS_NUDGE_PX)))
    }

    pub fn move_down(&mut self) -> bool {
        self.nudge_active(Point::from((0., CANVAS_NUDGE_PX)))
    }

    /// Moves a tile to an absolute canvas position (mouse drag), with animation.
    pub fn move_to(&mut self, idx: usize, new_pos: Point<f64, Logical>, animate: bool) {
        // Tiled windows don't move freely: dropping one onto another swaps their
        // slots, and dropping it anywhere else snaps it back.
        if self.canvas_tiling.is_on() {
            self.swap_tiling_slot_at(idx, new_pos);
            self.interactive_resize_end(None);
            return;
        }

        if animate {
            self.move_and_animate(idx, new_pos);
        } else {
            self.data[idx].pos = new_pos;
        }

        self.interactive_resize_end(None);
    }

    pub fn move_tile_to(&mut self, id: &W::Id, new_pos: Point<f64, Logical>, animate: bool) {
        let Some(idx) = self.idx_of(id) else {
            return;
        };
        self.move_to(idx, new_pos, animate);
    }

    fn move_and_animate(&mut self, idx: usize, new_pos: Point<f64, Logical>) {
        // Moves up to this canvas-unit distance are not animated.
        const ANIMATION_THRESHOLD_SQ: f64 = 10. * 10.;

        let tile = &mut self.tiles[idx];
        let data = &mut self.data[idx];

        let prev_pos = data.pos;
        data.pos = new_pos;

        let diff = prev_pos - new_pos;
        if diff.x * diff.x + diff.y * diff.y > ANIMATION_THRESHOLD_SQ {
            tile.animate_move_from(prev_pos - new_pos);
        }
    }

    /// Moves a tile to an absolute canvas position, in working-area-relative
    /// terms for IPC (`move-window-to-position` style callers).
    pub fn move_window(
        &mut self,
        id: Option<&W::Id>,
        x: PositionChange,
        y: PositionChange,
        animate: bool,
    ) {
        let Some(id) = id.or(self.active_window_id.as_ref()).cloned() else {
            return;
        };
        let Some(idx) = self.idx_of(&id) else {
            return;
        };

        // Position changes are relative to the visible view, like vxwm's
        // monitor-relative moves.
        let (ox, oy) = self.canvas_viewport.visible_origin();
        let (vw, vh) = self.canvas_viewport.visible_size();

        const MAX_F: f64 = 10000.;

        let mut pos = self.data[idx].pos;
        match x {
            PositionChange::SetFixed(x) => pos.x = ox + x,
            PositionChange::SetProportion(prop) => {
                let prop = (prop / 100.).clamp(0., MAX_F);
                pos.x = ox + vw * prop;
            }
            PositionChange::AdjustFixed(x) => pos.x += x,
            PositionChange::AdjustProportion(prop) => {
                let current_prop = (pos.x - ox) / vw.max(1.);
                let prop = (current_prop + prop / 100.).clamp(0., MAX_F);
                pos.x = ox + vw * prop;
            }
        }
        match y {
            PositionChange::SetFixed(y) => pos.y = oy + y,
            PositionChange::SetProportion(prop) => {
                let prop = (prop / 100.).clamp(0., MAX_F);
                pos.y = oy + vh * prop;
            }
            PositionChange::AdjustProportion(prop) => {
                let current_prop = (pos.y - oy) / vh.max(1.);
                let prop = (current_prop + prop / 100.).clamp(0., MAX_F);
                pos.y = oy + vh * prop;
            }
            PositionChange::AdjustFixed(y) => pos.y += y,
        }

        self.move_to(idx, pos, animate);
    }

    /// Centers the camera on a window (rather than moving the window).
    pub fn center_on_window(&mut self, window: &W::Id) {
        // Frozen while exclusive fullscreen owns the output.
        if self.fullscreen_lock() {
            return;
        }

        // While tiling is on the whole layout already fills the screen: chasing
        // individual windows would drag the tiled cluster out of view, so put the
        // camera back over the tiling region instead.
        if self.canvas_tiling.is_on() && self.idx_of(window).is_some() {
            self.center_on_tiling_region();
            return;
        }

        let Some(idx) = self.idx_of(window) else {
            return;
        };
        let rect = self.data[idx].rect();
        self.canvas_viewport
            .center_on_rect(rect.loc.x, rect.loc.y, rect.size.w, rect.size.h);
    }

    pub fn center_on_active(&mut self) {
        let Some(id) = self.active_window_id.clone() else {
            return;
        };
        self.center_on_window(&id);
    }

    /// Tile width preset cycling for the active tile.
    /// Toggles the active tile between its current width and the full working width.
    pub fn toggle_full_width(&mut self) {
        let Some(id) = self.active_window_id.clone() else {
            return;
        };
        let idx = self.idx_of(&id).unwrap();

        if let Some(restore) = self.data[idx].full_width_restore.take() {
            // Leave full width: restore the previous tile width.
            let tile = &mut self.tiles[idx];
            let height = tile.window().expected_size().unwrap_or_default().h;
            let width = tile.window_width_for_tile_width(restore);
            let width = width.round().clamp(1., 100000.) as i32;
            let win_size = Size::from((width, height));
            tile.window_mut().request_size_once(win_size, true);
        } else {
            // Enter full width: remember the current width and expand.
            let tile = &mut self.tiles[idx];
            let current = tile.tile_expected_or_current_size().w;
            self.data[idx].full_width_restore = Some(current);
            let height = tile.window().expected_size().unwrap_or_default().h;
            let width = tile.window_width_for_tile_width(self.working_area.size.w);
            let width = width.round().clamp(1., 100000.) as i32;
            let win_size = Size::from((width, height));
            tile.window_mut().request_size_once(win_size, true);
        }

        self.interactive_resize_end(Some(&id));
    }

    pub fn set_window_width(&mut self, window: Option<&W::Id>, change: SizeChange) {
        let Some(id) = window.or(self.active_window_id.as_ref()).cloned() else {
            return;
        };
        let Some(idx) = self.idx_of(&id) else {
            return;
        };

        self.data[idx].preset_width_idx = None;
        self.data[idx].full_width_restore = None;
        self.cancel_interactive_resize_for(&id);

        // Tile width, borders included.
        let available_size = self.working_area.size.w;
        let tile = &self.tiles[idx];
        let current_tile = tile.tile_expected_or_current_size().w;

        const MAX_F: f64 = 10000.;

        let tile_width = match change {
            SizeChange::SetFixed(tile_width) => f64::from(tile_width),
            SizeChange::SetProportion(prop) => {
                let prop = (prop / 100.).clamp(0., MAX_F);
                available_size * prop
            }
            SizeChange::AdjustFixed(delta) => current_tile + f64::from(delta),
            SizeChange::AdjustProportion(delta) => {
                let current_prop = current_tile / available_size;
                let prop = (current_prop + delta / 100.).clamp(0., MAX_F);
                available_size * prop
            }
        };
        self.request_tile_width(idx, tile_width);
    }

    /// Requests an absolute tile width (borders included) without touching
    /// preset state or an ongoing interactive resize.
    fn request_tile_width(&mut self, idx: usize, tile_width: f64) {
        const MAX_PX: f64 = 100000.;

        let tile = &self.tiles[idx];
        let win_width = tile.window_width_for_tile_width(tile_width);
        let win_width = win_width.round().clamp(1., MAX_PX) as i32;

        let tile = &mut self.tiles[idx];
        let win = tile.window_mut();
        let min_size = win.min_size();
        let max_size = win.max_size();

        let win_width = ensure_min_max_size(win_width, min_size.w, max_size.w);

        let win_height = win.expected_size().unwrap_or_default().h;
        let win_height = ensure_min_max_size(win_height, min_size.h, max_size.h);

        let win_size = Size::from((win_width, win_height));
        win.request_size_once(win_size, true);
    }

    pub fn set_window_height(&mut self, window: Option<&W::Id>, change: SizeChange) {
        let Some(id) = window.or(self.active_window_id.as_ref()).cloned() else {
            return;
        };
        let Some(idx) = self.idx_of(&id) else {
            return;
        };

        self.data[idx].preset_height_idx = None;
        self.cancel_interactive_resize_for(&id);

        let available_size = self.working_area.size.h;
        let tile = &self.tiles[idx];
        let current_tile = tile.tile_expected_or_current_size().h;

        const MAX_F: f64 = 10000.;

        let tile_height = match change {
            SizeChange::SetFixed(tile_height) => f64::from(tile_height),
            SizeChange::SetProportion(prop) => {
                let prop = (prop / 100.).clamp(0., MAX_F);
                available_size * prop
            }
            SizeChange::AdjustFixed(delta) => current_tile + f64::from(delta),
            SizeChange::AdjustProportion(delta) => {
                let current_prop = current_tile / available_size;
                let prop = (current_prop + delta / 100.).clamp(0., MAX_F);
                available_size * prop
            }
        };
        self.request_tile_height(idx, tile_height);
    }

    /// Requests an absolute tile height without touching preset state or an
    /// ongoing interactive resize.
    fn request_tile_height(&mut self, idx: usize, tile_height: f64) {
        const MAX_PX: f64 = 100000.;

        let tile = &self.tiles[idx];
        let win_height = tile.window_height_for_tile_height(tile_height);
        let win_height = win_height.round().clamp(1., MAX_PX) as i32;

        let tile = &mut self.tiles[idx];
        let win = tile.window_mut();
        let min_size = win.min_size();
        let max_size = win.max_size();

        let win_height = ensure_min_max_size(win_height, min_size.h, max_size.h);

        let win_width = win.expected_size().unwrap_or_default().w;
        let win_width = ensure_min_max_size(win_width, min_size.w, max_size.w);

        let win_size = Size::from((win_width, win_height));
        win.request_size_once(win_size, true);
    }

    /// Resets the tile height back to automatic (client-picked).
    pub fn reset_window_height(&mut self, window: Option<&W::Id>) {
        let Some(id) = window.or(self.active_window_id.as_ref()).cloned() else {
            return;
        };
        let Some(idx) = self.idx_of(&id) else {
            return;
        };

        self.data[idx].preset_height_idx = None;

        let tile = &mut self.tiles[idx];
        let win = tile.window_mut();
        let min_size = win.min_size();
        let max_size = win.max_size();

        let win_width = win.expected_size().unwrap_or_default().w;
        let win_width = ensure_min_max_size(win_width, min_size.w, max_size.w);

        // Height 0 asks the client to pick its natural height.
        let win_height = ensure_min_max_size_maybe_zero(0, min_size.h, max_size.h);

        let win_size = Size::from((win_width, win_height));
        win.request_size_once(win_size, true);
    }

    pub fn toggle_window_width(&mut self, window: Option<&W::Id>, forwards: bool) {
        let Some(id) = window.or(self.active_window_id.as_ref()).cloned() else {
            return;
        };
        let Some(idx) = self.idx_of(&id) else {
            return;
        };

        let available_size = self.working_area.size.w;

        let len = self.options.layout.preset_window_widths.len();
        let tile = &mut self.tiles[idx];
        let preset_idx = if let Some(idx) = self.data[idx].preset_width_idx {
            (idx + if forwards { 1 } else { len - 1 }) % len
        } else {
            let current_window = tile.window_expected_or_current_size().w;
            let current_tile = tile.tile_expected_or_current_size().w;

            let mut it = self
                .options
                .layout
                .preset_window_widths
                .iter()
                .map(|preset| resolve_preset_size(*preset, available_size));

            if forwards {
                it.position(|resolved| {
                    match resolved {
                        // Some allowance for fractional scaling purposes.
                        ResolvedSize::Tile(resolved) => current_tile + 1. < resolved,
                        ResolvedSize::Window(resolved) => current_window + 1. < resolved,
                    }
                })
                .unwrap_or(0)
            } else {
                it.rposition(|resolved| {
                    match resolved {
                        // Some allowance for fractional scaling purposes.
                        ResolvedSize::Tile(resolved) => resolved + 1. < current_tile,
                        ResolvedSize::Window(resolved) => resolved + 1. < current_window,
                    }
                })
                .unwrap_or(len - 1)
            }
        };

        let preset = self.options.layout.preset_window_widths[preset_idx];
        self.set_window_width(Some(&id), SizeChange::from(preset));
        self.data[idx].preset_width_idx = Some(preset_idx);

        self.interactive_resize_end(Some(&id));
    }

    pub fn toggle_window_height(&mut self, window: Option<&W::Id>, forwards: bool) {
        let Some(id) = window.or(self.active_window_id.as_ref()).cloned() else {
            return;
        };
        let Some(idx) = self.idx_of(&id) else {
            return;
        };

        let available_size = self.working_area.size.h;

        let len = self.options.layout.preset_window_heights.len();
        let tile = &mut self.tiles[idx];
        let preset_idx = if let Some(idx) = self.data[idx].preset_height_idx {
            (idx + if forwards { 1 } else { len - 1 }) % len
        } else {
            let current_window = tile.window_expected_or_current_size().h;
            let current_tile = tile.tile_expected_or_current_size().h;

            let mut it = self
                .options
                .layout
                .preset_window_heights
                .iter()
                .map(|preset| resolve_preset_size(*preset, available_size));

            if forwards {
                it.position(|resolved| {
                    match resolved {
                        // Some allowance for fractional scaling purposes.
                        ResolvedSize::Tile(resolved) => current_tile + 1. < resolved,
                        ResolvedSize::Window(resolved) => current_window + 1. < resolved,
                    }
                })
                .unwrap_or(0)
            } else {
                it.rposition(|resolved| {
                    match resolved {
                        // Some allowance for fractional scaling purposes.
                        ResolvedSize::Tile(resolved) => resolved + 1. < current_tile,
                        ResolvedSize::Window(resolved) => resolved + 1. < current_window,
                    }
                })
                .unwrap_or(len - 1)
            }
        };

        let preset = self.options.layout.preset_window_heights[preset_idx];
        self.set_window_height(Some(&id), SizeChange::from(preset));
        self.data[idx].preset_height_idx = Some(preset_idx);

        self.interactive_resize_end(Some(&id));
    }

    fn cancel_interactive_resize_for(&mut self, window: &W::Id) {
        if let Some(resize) = &self.interactive_resize {
            if &resize.window == window {
                self.interactive_resize = None;
            }
        }

        if let Some(idx) = self.idx_of(window) {
            self.tiles[idx].window_mut().cancel_interactive_resize();
        }
    }

    pub fn set_fullscreen(&mut self, window: &W::Id, is_fullscreen: bool) -> bool {
        let Some(idx) = self.idx_of(window) else {
            return false;
        };

        let pending_fullscreen = self.tiles[idx]
            .window()
            .pending_sizing_mode()
            .is_fullscreen();
        if is_fullscreen == pending_fullscreen {
            return false;
        }

        self.cancel_interactive_resize_for(window);

        if is_fullscreen {
            // Remember the committed size to restore on unfullscreen: that's
            // what's actually on screen.
            let size = self.tiles[idx].window().size();
            self.data[idx].unfs_restore = Some(size);
            self.tiles[idx].request_fullscreen(true, None);

            // Exclusive fullscreen owns the whole output: leave canvas
            // overview immediately (it would otherwise soft-lock the camera).
            if self.canvas_viewport.in_overview {
                self.canvas_viewport
                    .toggle_overview(self.options.canvas.overview_scale);
                self.canvas_viewport.snap_to_targets();
            }

            // Remember the camera to restore on exit (outermost enter only).
            if self.saved_view.is_none() {
                self.saved_view = Some(self.canvas_viewport.target());
            }
        } else {
            // Without a remembered size (e.g. opened fullscreen), keep the
            // current size: the important part is leaving the state, which
            // must always send a configure or the window sticks forever.
            let size = self.data[idx]
                .unfs_restore
                .take()
                .unwrap_or_else(|| self.tiles[idx].window().size());
            let tile = &mut self.tiles[idx];
            tile.window_mut()
                .request_size(size, SizingMode::Normal, true, None);

            // Restore the pre-fullscreen camera once no fullscreen tile
            // remains, so navigation resumes exactly where it was.
            let any_fullscreen = self
                .tiles
                .iter()
                .any(|t| t.window().pending_sizing_mode().is_fullscreen());
            if !any_fullscreen {
                if let Some((x, y, z)) = self.saved_view.take() {
                    self.canvas_viewport.restore_targets(x, y, z);
                }
            }
        }

        // Fullscreen windows are excluded from tiling: the rest reflows into the
        // whole region, and on exit the window slots back in.
        if self.canvas_tiling.is_on() {
            self.retile();
        }

        // Fullscreen windows render above the top layer, in screen space.
        true
    }

    /// Exclusive-fullscreen navigation lock.
    ///
    /// True while the active window is entering or in fullscreen: the camera
    /// must stay frozen and canvas navigation is blocked until exit.
    pub fn fullscreen_lock(&self) -> bool {
        let Some(id) = &self.active_window_id else {
            return false;
        };
        self.tiles
            .iter()
            .find(|tile| tile.window().id() == id)
            .is_some_and(|tile| tile.window().pending_sizing_mode().is_fullscreen())
    }

    /// Tile for the exclusive screen-space fullscreen pass.
    ///
    /// The active tile once it is committed fullscreen and the camera is
    /// stationary. The canvas pass skips it; it is drawn separately at exact
    /// output size, unscaled, above all layer-shell.
    pub fn exclusive_fullscreen_tile(&self) -> Option<&Tile<W>> {
        if self.canvas_viewport.is_animating() {
            return None;
        }
        let id = self.active_window_id.as_ref()?;
        self.tiles
            .iter()
            .find(|tile| tile.window().id() == id)
            .filter(|tile| tile.window().sizing_mode().is_fullscreen())
    }

    pub fn set_maximized(&mut self, window: &W::Id, maximize: bool) -> bool {
        let Some(idx) = self.idx_of(window) else {
            return false;
        };

        let pending_maximized = self.tiles[idx]
            .window()
            .pending_sizing_mode()
            .is_maximized();
        if maximize == pending_maximized {
            return false;
        }

        self.cancel_interactive_resize_for(window);

        if maximize {
            let size = self.tiles[idx].window().size();
            self.data[idx].unfs_restore = Some(size);
            let area = self.working_area.size;
            self.tiles[idx].request_maximized(area, true, None);

            // Same as fullscreen: bring the camera onto the window at 1:1 so
            // the maximized tile actually fills the monitor. While tiling is on
            // the camera stays pinned to the tiling region instead.
            if !self.canvas_tiling.is_on() {
                let tile = &self.tiles[idx];
                let size = tile.tile_expected_or_current_size();
                let pos = self.data[idx].pos;
                self.canvas_viewport
                    .center_on_rect(pos.x, pos.y, size.w, size.h);
                self.canvas_viewport.reset_zoom();
            }
        } else {
            // Same fallback as unfullscreening: always send a configure.
            let size = self.data[idx]
                .unfs_restore
                .take()
                .unwrap_or_else(|| self.tiles[idx].window().size());
            let tile = &mut self.tiles[idx];
            tile.window_mut()
                .request_size(size, SizingMode::Normal, true, None);
        }

        // Same as fullscreen: maximized windows step out of the tiling layout.
        if self.canvas_tiling.is_on() {
            self.retile();
            self.center_on_tiling_region();
        }

        true
    }

    pub fn render_above_top_layer(&self) -> bool {
        // Render above the top layer on a stationary fullscreen window.
        if self.canvas_viewport.is_animating() {
            return false;
        }

        let Some(id) = &self.active_window_id else {
            return false;
        };
        self.tiles
            .iter()
            .find(|tile| tile.window().id() == id)
            .is_some_and(|tile| tile.window().sizing_mode().is_fullscreen())
    }

    pub fn render<R: NiriRenderer>(
        &self,
        mut ctx: RenderCtx<R>,
        xray_pos: XrayPos,
        focus_ring: bool,
        layer: RenderLayer,
        ws_alpha: f32,
        push: &mut dyn FnMut(CanvasSpaceRenderElement<R>),
    ) {
        let scale = Scale::from(self.scale);

        // Draw the closing windows on top of the other windows.
        if layer.is_normal() {
            let (ox, oy) = self.canvas_viewport.visible_origin();
            let (vw, vh) = self.canvas_viewport.visible_size();
            let view_rect = Rectangle::new(Point::from((ox, oy)), Size::from((vw, vh)));
            for closing in self.closing_windows.iter().rev() {
                let elem = closing.render(ctx.as_gles(), view_rect, scale);
                push(elem.into());
            }
        }

        if self.tiles.is_empty() {
            return;
        }

        let active = self.active_window_id.clone();

        // The exclusive fullscreen tile is drawn by a separate screen-space
        // pass (exact output size, above all layer-shell); skip it here so it
        // is not rendered twice through the camera transform.
        let exclusive_id = self
            .exclusive_fullscreen_tile()
            .map(|t| t.window().id().clone());

        // Tiles are stored topmost-first; push in storage order so closer
        // windows composite on top (same convention as FloatingSpace).
        // This matches self.tiles_with_render_positions().
        for (tile, data) in zip(&self.tiles, &self.data) {
            // Skip tiles belonging to a different render layer.
            if layer.is_normal() == tile.is_moving_between_workspaces() {
                continue;
            }

            if layer.is_normal() && Some(tile.window().id()) == exclusive_id.as_ref() {
                continue;
            }

            // For the active tile, draw the focus ring.
            let focus_ring = focus_ring && Some(tile.window().id()) == active.as_ref();

            // Position in canvas coordinates; the monitor folds the camera
            // zoom and pan into its rescale + relocate step.
            let tile_pos = data.pos + tile.render_offset();

            let xray_pos = xray_pos.offset(tile_pos);
            tile.render(ctx.r(), tile_pos, xray_pos, focus_ring, ws_alpha, &mut |elem| {
                push(elem.into())
            });
        }
    }

    /// Tiles with their canvas positions (absolute R² coordinates).
    pub fn tiles_with_canvas_positions(
        &self,
    ) -> impl Iterator<Item = (&Tile<W>, Point<f64, Logical>)> + '_ {
        zip(&self.tiles, &self.data).map(|(tile, data)| (tile, data.pos))
    }

    /// Tiles with workspace-view positions (camera transform applied, 1:1 size).
    ///
    /// Sizes are NOT scaled here: the monitor applies the camera zoom to the
    /// whole layer via rescale + relocate. Positions are canvas transformed to
    /// view space for hit-testing and IPC.
    pub fn tiles_with_render_positions(
        &self,
    ) -> impl Iterator<Item = (&Tile<W>, Point<f64, Logical>, bool)> {
        let viewport = &self.canvas_viewport;
        zip(&self.tiles, &self.data).map(move |(tile, data)| {
            let pos = data.pos + tile.render_offset();
            let (sx, sy) = viewport.canvas_to_screen(pos.x, pos.y);
            (tile, Point::from((sx, sy)), true)
        })
    }

    pub fn tiles_with_render_positions_mut(
        &mut self,
        _round: bool,
    ) -> impl Iterator<Item = (&mut Tile<W>, Point<f64, Logical>)> {
        let viewport = &self.canvas_viewport;
        zip(&mut self.tiles, &self.data).map(move |(tile, data)| {
            let pos = data.pos + tile.render_offset();
            let (sx, sy) = viewport.canvas_to_screen(pos.x, pos.y);
            (tile, Point::from((sx, sy)))
        })
    }

    pub fn tiles_with_ipc_layouts(&self) -> impl Iterator<Item = (&Tile<W>, WindowLayout)> {
        let viewport = &self.canvas_viewport;
        zip(&self.tiles, &self.data).map(move |(tile, data)| {
            // Do not include animated render offset here to avoid IPC spam.
            let pos = data.pos;
            let (sx, sy) = viewport.canvas_to_screen(pos.x, pos.y);

            let layout = WindowLayout {
                pos_in_canvas_layout: Some((pos.x, pos.y)),
                tile_pos_in_workspace_view: Some((sx, sy)),
                ..tile.ipc_layout_template()
            };
            (tile, layout)
        })
    }

    /// Visual rectangle of the active window, in workspace-view coordinates.
    pub fn active_window_visual_rectangle(&self) -> Option<Rectangle<f64, Logical>> {
        let id = self.active_window_id.as_ref()?;
        let idx = self.idx_of(id)?;
        let tile = &self.tiles[idx];

        let canvas_pos = self.data[idx].pos + tile.window_loc();
        let window_size = tile.window_size();
        let (sx, sy) = self
            .canvas_viewport
            .canvas_to_screen(canvas_pos.x, canvas_pos.y);
        let zoom = self.canvas_viewport.zoom;
        let window_rect = Rectangle::new(
            Point::from((sx, sy)),
            Size::from((window_size.w * zoom, window_size.h * zoom)),
        );

        let (ox, oy) = self.canvas_viewport.visible_origin();
        let (vw, vh) = self.canvas_viewport.visible_size();
        let (vx, vy) = self.canvas_viewport.canvas_to_screen(ox, oy);
        let visible = Rectangle::new(Point::from((vx, vy)), Size::from((vw * zoom, vh * zoom)));

        window_rect.intersection(visible)
    }

    pub fn popup_target_rect(&self, id: &W::Id) -> Option<Rectangle<f64, Logical>> {
        let idx = self.idx_of(id)?;
        let tile = &self.tiles[idx];

        // Bounds in tile-local canvas units, converted to view units.
        let zoom = self.canvas_viewport.zoom;
        let (ox, oy) = self.canvas_viewport.visible_origin();
        let (vw, vh) = self.canvas_viewport.visible_size();

        let mut target = Rectangle::new(Point::from((ox, oy)), Size::from((vw, vh)));
        target.loc -= self.data[idx].pos;
        target.loc -= tile.window_loc();

        // Tile-local canvas units -> tile-local view units.
        target.loc = target.loc.upscale(zoom);
        target.size = target.size.upscale(zoom);

        Some(target)
    }

    pub fn window_under(&self, pos: Point<f64, Logical>) -> Option<(&W, HitType)> {
        // `pos` is in workspace-view coordinates; map to canvas first.
        let zoom = self.canvas_viewport.zoom.max(1e-6);
        let (cx, cy) = self.canvas_viewport.screen_to_canvas(pos.x, pos.y);
        let canvas_point = Point::from((cx, cy));

        // Topmost first.
        for (tile, data) in zip(&self.tiles, &self.data) {
            let origin_canvas = data.pos + tile.render_offset();
            // Already in canvas units (screen_to_canvas divided by the zoom);
            // tiles live 1:1 in canvas space, so no further scaling here.
            let local = canvas_point - origin_canvas;
            if let Some(hit) = tile.hit(local) {
                // Report back in workspace-view coordinates, with the camera
                // zoom attached: the tile-local buffer origin scales with the
                // zoom, so pointer mapping must divide by it to recover exact
                // surface-local coordinates at any zoom level.
                let (sx, sy) = self
                    .canvas_viewport
                    .canvas_to_screen(origin_canvas.x, origin_canvas.y);
                let origin_view = Point::from((sx, sy));
                let hit = match hit {
                    HitType::Input { win_pos, .. } => HitType::Input {
                        win_pos: origin_view + win_pos.upscale(zoom),
                        scale: zoom,
                    },
                    hit => hit,
                };
                // Live transform diagnostics (Bug #2 hunt): compare with the
                // matching [render]/[map] lines. Enable with NYX_DEBUG_XFORM=1.
                if std::env::var("NYX_DEBUG_XFORM").is_ok() {
                    eprintln!(
                        "[nyx-xform][hit]   cam=({:.3},{:.3}) zoom={:.4} query=({:.1},{:.1}) \
                         origin_canvas=({:.1},{:.1}) hit={:?}",
                        self.canvas_viewport.cam_x,
                        self.canvas_viewport.cam_y,
                        self.canvas_viewport.zoom,
                        pos.x,
                        pos.y,
                        origin_canvas.x,
                        origin_canvas.y,
                        hit,
                    );
                }
                return Some((tile.window(), hit));
            }
        }

        None
    }

    pub fn resize_edges_under(&self, pos: Point<f64, Logical>) -> Option<ResizeEdge> {
        // Tiled windows are sized by the layout, so don't offer resize edges
        // (and the resize cursor) that would be undone by the next re-tile.
        if self.canvas_tiling.is_on() {
            return None;
        }

        let (cx, cy) = self.canvas_viewport.screen_to_canvas(pos.x, pos.y);
        let canvas_point = Point::from((cx, cy));

        for (tile, data) in zip(&self.tiles, &self.data) {
            let origin_canvas = data.pos + tile.render_offset();
            // Canvas units, 1:1 with tile space (see window_under()).
            let pos_within_tile = canvas_point - origin_canvas;

            if tile.hit(pos_within_tile).is_some() {
                let size = tile.tile_size().to_f64();

                let mut edges = ResizeEdge::empty();
                if pos_within_tile.x < size.w / 3. {
                    edges |= ResizeEdge::LEFT;
                } else if 2. * size.w / 3. < pos_within_tile.x {
                    edges |= ResizeEdge::RIGHT;
                }
                if pos_within_tile.y < size.h / 3. {
                    edges |= ResizeEdge::TOP;
                } else if 2. * size.h / 3. < pos_within_tile.y {
                    edges |= ResizeEdge::BOTTOM;
                }
                return Some(edges);
            }
        }

        None
    }

    pub fn interactive_resize_begin(&mut self, window: W::Id, edges: ResizeEdge) -> bool {
        // The tiler owns window sizes while it is on.
        if self.canvas_tiling.is_on() {
            return false;
        }

        if self.interactive_resize.is_some() {
            return false;
        }

        let tile = self
            .tiles
            .iter_mut()
            .find(|tile| tile.window().id() == &window)
            .unwrap();

        let original_window_size = tile.window_size();

        let resize = InteractiveResize {
            window,
            original_window_size,
            data: InteractiveResizeData { edges },
        };
        self.interactive_resize = Some(resize);

        true
    }

    pub fn interactive_resize_update(
        &mut self,
        window: &W::Id,
        delta: Point<f64, Logical>,
    ) -> bool {
        let Some(resize) = &self.interactive_resize else {
            return false;
        };

        if window != &resize.window {
            return false;
        }

        let original_window_size = resize.original_window_size;
        let edges = resize.data.edges;

        // Pointer deltas arrive in workspace-view (zoomed) coordinates;
        // window sizes and positions live in canvas units.
        let zoom = self.canvas_viewport.zoom.max(1e-6);
        let delta = delta.downscale(zoom);

        let Some(idx) = self.idx_of(window) else {
            return false;
        };

        if edges.intersects(ResizeEdge::LEFT_RIGHT) {
            let mut dx = delta.x;
            if edges.contains(ResizeEdge::LEFT) {
                dx = -dx;
                // Keep the grabbed edge under the cursor.
                self.data[idx].pos.x += delta.x;
            };

            // Bypass set_window_width(): it cancels the interactive resize,
            // which would end the gesture after a single step.
            self.data[idx].preset_width_idx = None;
            self.data[idx].full_width_restore = None;
            self.request_tile_width(idx, original_window_size.w + dx);
        }

        if edges.intersects(ResizeEdge::TOP_BOTTOM) {
            let mut dy = delta.y;
            if edges.contains(ResizeEdge::TOP) {
                dy = -dy;
                // Keep the grabbed edge under the cursor.
                self.data[idx].pos.y += delta.y;
            };

            // Bypass set_window_height() for the same reason as above.
            self.data[idx].preset_height_idx = None;
            self.request_tile_height(idx, original_window_size.h + dy);
        }

        true
    }

    pub fn interactive_resize_end(&mut self, window: Option<&W::Id>) {
        let Some(resize) = &self.interactive_resize else {
            return;
        };

        if let Some(window) = window {
            if window != &resize.window {
                return;
            }
        }

        self.interactive_resize = None;
    }

    pub fn refresh(&mut self, is_active: bool, is_focused: bool) {
        let active = self.active_window_id.clone();
        for tile in &mut self.tiles {
            let win = tile.window_mut();

            win.set_active_in_tile(true);
            win.set_floating(false);

            let mut is_active = is_active && Some(win.id()) == active.as_ref();
            if self.options.deactivate_unfocused_windows {
                is_active &= is_focused;
            }
            win.set_activated(is_active);

            let resize_data = self
                .interactive_resize
                .as_ref()
                .filter(|resize| &resize.window == win.id())
                .map(|resize| resize.data);
            win.set_interactive_resize(resize_data);

            let border_config = self.options.layout.border.merged_with(&win.rules().border);
            let bounds = compute_toplevel_bounds(border_config, self.working_area.size);
            win.set_bounds(bounds);

            // If transactions are disabled, also disable combined throttling, for more
            // intuitive behavior.
            let intent = if self.options.disable_resize_throttling {
                ConfigureIntent::CanSend
            } else {
                win.configure_intent()
            };

            if matches!(
                intent,
                ConfigureIntent::CanSend | ConfigureIntent::ShouldSend
            ) {
                win.send_pending_configure();
            }

            win.refresh();
        }
    }

    /// Fraction of the visible width that the camera must pan to reveal a window.
    ///
    /// Used by focus-follows-mouse: 0 means fully visible.
    pub fn scroll_amount_to_activate(&self, window: &W::Id) -> f64 {
        let Some(idx) = self.idx_of(window) else {
            return 0.;
        };

        if Some(window) == self.active_window_id.as_ref() {
            return 0.;
        }

        let rect = self.data[idx].rect();
        let (ox, oy) = self.canvas_viewport.visible_origin();
        let (vw, vh) = self.canvas_viewport.visible_size();
        let visible = Rectangle::new(Point::from((ox, oy)), Size::from((vw, vh)));

        if visible.contains_rect(rect) {
            return 0.;
        }

        // Minimal pan distance to bring the rect into view, relative to width.
        let dx = if rect.loc.x + rect.size.w <= ox {
            ox - (rect.loc.x + rect.size.w)
        } else if rect.loc.x >= ox + vw {
            rect.loc.x - (ox + vw)
        } else {
            0.
        };
        let dy = if rect.loc.y + rect.size.h <= oy {
            oy - (rect.loc.y + rect.size.h)
        } else if rect.loc.y >= oy + vh {
            rect.loc.y - (oy + vh)
        } else {
            0.
        };

        (dx * dx + dy * dy).sqrt() / vw.max(1.)
    }

    // Nyx infinite-canvas camera API (spec §1B–1D).

    pub fn canvas_viewport(&self) -> &Viewport {
        &self.canvas_viewport
    }

    pub fn canvas_viewport_mut(&mut self) -> &mut Viewport {
        &mut self.canvas_viewport
    }

    pub fn canvas_tiling(&self) -> &DynamicTiling {
        &self.canvas_tiling
    }

    pub fn is_tiling(&self) -> bool {
        self.canvas_tiling.is_on()
    }

    /// Flips dynamic tiling on and off.
    ///
    /// Turning it on lays every window out like a classic tiler inside the chunk
    /// of canvas currently under the working area. Turning it off gives every
    /// window back the exact position and size it had on the free canvas before.
    pub fn toggle_dynamic_tiling(&mut self) {
        let kind = if self.canvas_tiling.is_on() {
            TilingKind::Off
        } else {
            self.preferred_tiling_kind()
        };
        self.set_tiling_kind(kind);
    }

    /// Layout to switch to when tiling is toggled on: whatever the config asks
    /// for, master-stack by default.
    fn preferred_tiling_kind(&self) -> TilingKind {
        use nyx_config::canvas::CanvasTilingKind as ConfigKind;

        match self.options.canvas.tiling.kind {
            ConfigKind::Off | ConfigKind::MasterStack => TilingKind::MasterStack,
            ConfigKind::Grid => TilingKind::Grid,
            ConfigKind::Dwindle => TilingKind::Dwindle,
        }
    }

    /// Switches to a specific tiling layout (or [`TilingKind::Off`]).
    pub fn set_tiling_kind(&mut self, kind: TilingKind) {
        if self.canvas_tiling.kind == kind {
            return;
        }

        let was_on = self.canvas_tiling.is_on();
        self.canvas_tiling.kind = kind;
        self.tiling_override = Some(kind);

        if kind.is_on() {
            if !was_on {
                // Tiling means "fill the screen", so start from a 1:1 camera.
                self.canvas_viewport.reset_zoom();
                self.anchor_tiling_region();
            }
            self.sync_tiling_order();
            self.retile();
            self.center_on_tiling_region();
        } else {
            self.untile_all();
        }

        self.interactive_resize_end(None);
    }

    /// Cycles through the available tiling layouts (master-stack → grid →
    /// dwindle → master-stack), leaving tiling off untouched.
    pub fn cycle_tiling_kind(&mut self) {
        let next = match self.canvas_tiling.kind {
            TilingKind::Off => return,
            TilingKind::MasterStack => TilingKind::Grid,
            TilingKind::Grid => TilingKind::Dwindle,
            TilingKind::Dwindle => TilingKind::MasterStack,
        };
        self.set_tiling_kind(next);
    }

    /// Grows or shrinks the master area (Hyprland's `splitratio`).
    pub fn adjust_master_ratio(&mut self, delta: f64) {
        if !self.canvas_tiling.is_on() {
            return;
        }
        self.canvas_tiling.master_ratio = (self.canvas_tiling.master_ratio + delta).clamp(0.2, 0.8);
        self.retile();
    }

    /// Anchors the tiling region to the canvas chunk currently under the working
    /// area, using the camera's animation targets.
    fn anchor_tiling_region(&mut self) {
        let (target_x, target_y, target_zoom) = self.canvas_viewport.target();
        let zoom = target_zoom.max(1e-6);
        let out_w = self.canvas_viewport.output_w;
        let out_h = self.canvas_viewport.output_h;
        let area = self.working_area;

        let x = target_x + (area.loc.x - out_w / 2.) / zoom;
        let y = target_y + (area.loc.y - out_h / 2.) / zoom;
        let region = TilingRegion::new(x, y, area.size.w / zoom, area.size.h / zoom);

        self.tiling_region = Some(region);
        self.tiling_zoom = zoom;
    }

    /// Puts the camera back over the tiling region (exact inverse of
    /// [`anchor_tiling_region`](Self::anchor_tiling_region)).
    fn center_on_tiling_region(&mut self) {
        let Some(region) = self.tiling_region else {
            return;
        };

        let zoom = self.tiling_zoom.max(1e-6);
        let out_w = self.canvas_viewport.output_w;
        let out_h = self.canvas_viewport.output_h;
        let area = self.working_area;

        let cx = region.x + (out_w / 2. - area.loc.x) / zoom;
        let cy = region.y + (out_h / 2. - area.loc.y) / zoom;
        self.canvas_viewport.center_on(cx, cy);
    }

    /// Makes `tiling_order` hold exactly the windows of this space, once each.
    fn sync_tiling_order(&mut self) {
        let mut order = std::mem::take(&mut self.tiling_order);
        order.retain(|id| self.tiles.iter().any(|tile| tile.window().id() == id));

        for tile in &self.tiles {
            let id = tile.window().id();
            if !order.iter().any(|other| other == id) {
                order.push(id.clone());
            }
        }

        self.tiling_order = order;
    }

    /// Indices into `tiles` of the windows the tiler lays out, in slot order.
    ///
    /// Fullscreen and maximized windows step out of the layout: they own the
    /// whole output on their own.
    fn tiling_slots(&self) -> Vec<usize> {
        let takes_part =
            |tile: &Tile<W>| matches!(tile.window().pending_sizing_mode(), SizingMode::Normal);

        let mut slots = Vec::with_capacity(self.tiles.len());
        for id in &self.tiling_order {
            if let Some(idx) = self.idx_of(id) {
                if takes_part(&self.tiles[idx]) {
                    slots.push(idx);
                }
            }
        }

        // Anything missing from the order (shouldn't normally happen) goes last
        // rather than silently dropping out of the layout.
        for (idx, tile) in self.tiles.iter().enumerate() {
            if takes_part(tile) && !slots.contains(&idx) {
                slots.push(idx);
            }
        }

        slots
    }

    /// Lays every managed window out according to the current tiling policy.
    pub fn retile(&mut self) {
        if !self.canvas_tiling.is_on() {
            return;
        }

        if self.tiling_region.is_none() {
            self.anchor_tiling_region();
        }
        let Some(region) = self.tiling_region else {
            return;
        };

        let slots = self.tiling_slots();
        let count = slots.len();
        if count == 0 {
            return;
        }

        for (slot, idx) in slots.into_iter().enumerate() {
            let (x, y, w, h) = self.canvas_tiling.place_in(region, slot, count);

            // Remember where this window lived on the free canvas first.
            self.save_untiled_state(idx);

            self.request_tile_size(idx, w, h);
            self.move_and_animate(idx, Point::from((x, y)));
        }
    }

    /// Remembers the free-canvas position and size of a tile, once.
    fn save_untiled_state(&mut self, idx: usize) {
        if self.data[idx].untiled.is_some() {
            return;
        }

        let win = self.tiles[idx].window();
        let size = win.expected_size().unwrap_or_else(|| win.size());
        let win_size = Size::from((size.w.max(1), size.h.max(1)));

        self.data[idx].untiled = Some(UntiledState {
            pos: self.data[idx].pos,
            win_size,
        });
    }

    /// Gives every window back its pre-tiling position and size.
    fn untile_all(&mut self) {
        for idx in 0..self.tiles.len() {
            let Some(saved) = self.data[idx].untiled.take() else {
                continue;
            };

            {
                let win = self.tiles[idx].window_mut();
                let min_size = win.min_size();
                let max_size = win.max_size();
                let w = ensure_min_max_size(saved.win_size.w, min_size.w, max_size.w);
                let h = ensure_min_max_size(saved.win_size.h, min_size.h, max_size.h);
                win.request_size_once(Size::from((w, h)), true);
            }

            self.move_and_animate(idx, saved.pos);
        }

        self.tiling_region = None;
    }

    /// Requests an absolute tile size (borders included) for a tiled window.
    fn request_tile_size(&mut self, idx: usize, tile_width: f64, tile_height: f64) {
        const MAX_PX: f64 = 100000.;

        let tile = &self.tiles[idx];
        let win_width = tile.window_width_for_tile_width(tile_width);
        let win_width = win_width.round().clamp(1., MAX_PX) as i32;
        let win_height = tile.window_height_for_tile_height(tile_height);
        let win_height = win_height.round().clamp(1., MAX_PX) as i32;

        let tile = &mut self.tiles[idx];
        let win = tile.window_mut();
        let min_size = win.min_size();
        let max_size = win.max_size();

        let win_width = ensure_min_max_size(win_width, min_size.w, max_size.w);
        let win_height = ensure_min_max_size(win_height, min_size.h, max_size.h);

        win.request_size_once(Size::from((win_width, win_height)), true);
    }

    /// Swaps a window with its next / previous tiling slot.
    fn move_tiling_slot(&mut self, id: &W::Id, forwards: bool) -> bool {
        let Some(slot) = self.tiling_order.iter().position(|other| other == id) else {
            return false;
        };

        let other = if forwards {
            if slot + 1 >= self.tiling_order.len() {
                return false;
            }
            slot + 1
        } else {
            if slot == 0 {
                return false;
            }
            slot - 1
        };

        self.tiling_order.swap(slot, other);
        self.retile();
        self.interactive_resize_end(None);
        true
    }

    /// Drop handling for a tiled window: swap with whatever it was dropped onto,
    /// otherwise snap it back into its own slot.
    fn swap_tiling_slot_at(&mut self, idx: usize, new_pos: Point<f64, Logical>) {
        if idx >= self.tiles.len() {
            return;
        }

        let center = new_pos + self.data[idx].size.to_point().downscale(2.);

        let mut target = None;
        for (other_idx, data) in self.data.iter().enumerate() {
            if other_idx == idx {
                continue;
            }
            let rect = data.rect();
            if center.x >= rect.loc.x
                && center.x < rect.loc.x + rect.size.w
                && center.y >= rect.loc.y
                && center.y < rect.loc.y + rect.size.h
            {
                target = Some(other_idx);
                break;
            }
        }

        if let Some(other_idx) = target {
            let id = self.tiles[idx].window().id().clone();
            let other_id = self.tiles[other_idx].window().id().clone();

            let slot = self.tiling_order.iter().position(|other| *other == id);
            let other_slot = self.tiling_order.iter().position(|other| *other == other_id);
            if let (Some(slot), Some(other_slot)) = (slot, other_slot) {
                self.tiling_order.swap(slot, other_slot);
            }
        }

        self.retile();
    }

    /// Workspace-view coordinates of a canvas point.
    pub fn to_view_pos(&self, canvas: Point<f64, Logical>) -> Point<f64, Logical> {
        let (sx, sy) = self.canvas_viewport.canvas_to_screen(canvas.x, canvas.y);
        Point::from((sx, sy))
    }

    /// Canvas coordinates of a workspace-view point.
    pub fn to_canvas_pos(&self, view: Point<f64, Logical>) -> Point<f64, Logical> {
        let (cx, cy) = self.canvas_viewport.screen_to_canvas(view.x, view.y);
        Point::from((cx, cy))
    }

    /// Canvas navigation lock.
    ///
    /// While dynamic tiling is on, the layout is pinned to the tiling region and
    /// sized to fill the screen exactly. Panning, zooming or jumping home would
    /// just slide that region out from under the camera, so all free-canvas
    /// navigation is blocked until tiling is switched off again. Exclusive
    /// fullscreen locks navigation for the same reason.
    pub fn canvas_navigation_locked(&self) -> bool {
        self.canvas_tiling.is_on() || self.fullscreen_lock()
    }

    /// Key panning: move camera by canvas-space `(dx, dy)`.
    pub fn nyx_pan_by(&mut self, dx: f64, dy: f64) {
        if self.canvas_navigation_locked() {
            return;
        }
        self.canvas_viewport.pan_by(dx, dy);
    }

    /// Drag-panning with mouse (screen-space delta).
    pub fn nyx_drag_pan_by_screen(&mut self, dx: f64, dy: f64) {
        if self.canvas_navigation_locked() {
            return;
        }
        self.canvas_viewport.drag_pan_by_screen(dx, dy);
    }

    /// Pivot zoom around a screen point (Mod + wheel). Positive zooms in.
    pub fn nyx_zoom_by_notches(&mut self, notches: f64, pivot_screen: (f64, f64)) {
        if self.canvas_navigation_locked() {
            return;
        }
        self.canvas_viewport.zoom_by_notches(notches, pivot_screen);
    }

    pub fn nyx_reset_zoom(&mut self) {
        if self.canvas_navigation_locked() {
            return;
        }
        self.canvas_viewport.reset_zoom();
    }

    pub fn nyx_zoom_to(&mut self, scale: f64) {
        if self.canvas_navigation_locked() {
            return;
        }
        if !scale.is_finite() {
            return;
        }
        self.canvas_viewport.zoom_to(scale);
    }

    pub fn nyx_toggle_overview(&mut self) {
        if self.canvas_navigation_locked() {
            return;
        }
        self.canvas_viewport
            .toggle_overview(self.options.canvas.overview_scale);
    }

    pub fn nyx_home(&mut self) {
        if self.canvas_navigation_locked() {
            return;
        }
        self.canvas_viewport.home();
    }

    /// Center camera on the active tile (focus-tracking auto-centering).
    pub fn nyx_center_on_active(&mut self) {
        self.center_on_active();
    }

    pub fn view_size(&self) -> Size<f64, Logical> {
        self.view_size
    }

    pub fn parent_area(&self) -> Rectangle<f64, Logical> {
        self.parent_area
    }

    pub fn working_area(&self) -> Rectangle<f64, Logical> {
        self.working_area
    }

    pub fn clock(&self) -> &Clock {
        &self.clock
    }

    pub fn options(&self) -> &Rc<Options> {
        &self.options
    }

    #[cfg(test)]
    pub fn verify_invariants(&self) {
        assert!(self.scale > 0.);
        assert!(self.scale.is_finite());
        assert_eq!(self.tiles.len(), self.data.len());

        for (i, (tile, data)) in zip(&self.tiles, &self.data).enumerate() {
            assert!(Rc::ptr_eq(&self.options, &tile.options));
            assert_eq!(self.clock, tile.clock);
            assert_eq!(self.scale, tile.scale());
            tile.verify_invariants();

            if let Some(idx) = data.preset_width_idx {
                assert!(idx < self.options.layout.preset_window_widths.len());
            }
            if let Some(idx) = data.preset_height_idx {
                assert!(idx < self.options.layout.preset_window_heights.len());
            }

            let mut data2 = *data;
            data2.update(tile);
            // Position is ours to keep; only the cached size must track the tile.
            assert_eq!(data.size, data2.size, "tile data size must be up to date");

            for tile_below in &self.tiles[i + 1..] {
                assert!(
                    !tile_below.window().is_child_of(tile.window()),
                    "children must be stacked above parents"
                );
            }
        }

        if let Some(id) = &self.active_window_id {
            assert!(!self.tiles.is_empty());
            assert!(
                self.tiles.iter().any(|tile| tile.window().id() == id),
                "active window must be present in tiles"
            );
        } else {
            assert!(self.tiles.is_empty());
        }

        if let Some(resize) = &self.interactive_resize {
            assert!(
                self.has_window(&resize.window),
                "interactive resize window must be present in tiles"
            );
        }
    }
}

fn canvas_tiling_from_options(options: &Options) -> DynamicTiling {
    use nyx_config::canvas::CanvasTilingKind as ConfigKind;

    let (kind, enabled) = match options.canvas.tiling.kind {
        ConfigKind::Off => (TilingKind::Off, false),
        ConfigKind::MasterStack => (TilingKind::MasterStack, true),
        ConfigKind::Grid => (TilingKind::Grid, true),
        ConfigKind::Dwindle => (TilingKind::Dwindle, true),
    };
    let kind = if options.canvas.tiling.enabled && enabled {
        kind
    } else {
        TilingKind::Off
    };
    DynamicTiling::new(
        kind,
        options.canvas.tiling.gap,
        options.canvas.tiling.master_ratio,
    )
}

fn compute_working_area(
    parent_area: Rectangle<f64, Logical>,
    scale: f64,
    struts: Struts,
) -> Rectangle<f64, Logical> {
    let mut working_area = parent_area;

    working_area.size.w = f64::max(0., working_area.size.w - struts.left.0 - struts.right.0);
    working_area.loc.x += struts.left.0;

    working_area.size.h = f64::max(0., working_area.size.h - struts.top.0 - struts.bottom.0);
    working_area.loc.y += struts.top.0;

    let loc = working_area
        .loc
        .to_physical_precise_ceil(scale)
        .to_logical(scale);

    let mut size_diff = (loc - working_area.loc).to_size();
    size_diff.w = f64::min(working_area.size.w, size_diff.w);
    size_diff.h = f64::min(working_area.size.h, size_diff.h);

    working_area.size -= size_diff;
    working_area.loc = loc;

    working_area
}

fn compute_toplevel_bounds(
    border_config: nyx_config::Border,
    working_area_size: Size<f64, Logical>,
) -> Size<i32, Logical> {
    let mut border = 0.;
    if !border_config.off {
        border = border_config.width * 2.;
    }

    Size::from((
        f64::max(working_area_size.w - border, 1.),
        f64::max(working_area_size.h - border, 1.),
    ))
    .to_i32_floor()
}

fn resolve_preset_size(preset: PresetSize, view_size: f64) -> ResolvedSize {
    match preset {
        PresetSize::Proportion(proportion) => ResolvedSize::Tile(view_size * proportion),
        PresetSize::Fixed(width) => ResolvedSize::Window(f64::from(width)),
    }
}
