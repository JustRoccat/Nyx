//! Tests for the free 2D canvas layout.
//!
//! The old horizontal-strip tests were removed together with the strip: every
//! window is an independent node with absolute canvas coordinates, so these
//! tests verify positioning, spawning, spatial focus, nudging, sizing,
//! fullscreen and hit-testing instead.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use smithay::output;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{Logical, Point, Rectangle, Serial, Size, Transform};

use super::canvas_space::CanvasSpace;
use super::*;
use crate::utils::transaction::Transaction;

impl<W: LayoutElement> Default for Layout<W> {
    fn default() -> Self {
        Self::with_options(Clock::with_time(Duration::ZERO), Default::default())
    }
}

#[derive(Debug)]
struct TestWindowInner {
    id: usize,
    parent_id: Cell<Option<usize>>,
    bbox: Cell<Rectangle<i32, Logical>>,
    initial_bbox: Rectangle<i32, Logical>,
    requested_size: Cell<Option<Size<i32, Logical>>>,
    // Emulates the window ignoring the compositor-provided size.
    forced_size: Cell<Option<Size<i32, Logical>>>,
    min_size: Size<i32, Logical>,
    max_size: Size<i32, Logical>,
    pending_sizing_mode: Cell<SizingMode>,
    pending_activated: Cell<bool>,
    sizing_mode: Cell<SizingMode>,
    is_windowed_fullscreen: Cell<bool>,
    is_pending_windowed_fullscreen: Cell<bool>,
    animate_next_configure: Cell<bool>,
    animation_snapshot: RefCell<Option<LayoutElementRenderSnapshot>>,
    rules: ResolvedWindowRules,
}

#[derive(Debug, Clone)]
struct TestWindow(Rc<TestWindowInner>);

#[derive(Debug, Clone)]
struct TestWindowParams {
    id: usize,
    parent_id: Option<usize>,
    bbox: Rectangle<i32, Logical>,
    min_max_size: (Size<i32, Logical>, Size<i32, Logical>),
    rules: Option<ResolvedWindowRules>,
}

impl TestWindowParams {
    pub fn new(id: usize) -> Self {
        Self {
            id,
            parent_id: None,
            bbox: Rectangle::from_size(Size::from((100, 200))),
            min_max_size: Default::default(),
            rules: None,
        }
    }
}

impl TestWindow {
    fn new(params: TestWindowParams) -> Self {
        Self(Rc::new(TestWindowInner {
            id: params.id,
            parent_id: Cell::new(params.parent_id),
            bbox: Cell::new(params.bbox),
            initial_bbox: params.bbox,
            requested_size: Cell::new(None),
            forced_size: Cell::new(None),
            min_size: params.min_max_size.0,
            max_size: params.min_max_size.1,
            pending_sizing_mode: Cell::new(SizingMode::Normal),
            pending_activated: Cell::new(false),
            sizing_mode: Cell::new(SizingMode::Normal),
            is_windowed_fullscreen: Cell::new(false),
            is_pending_windowed_fullscreen: Cell::new(false),
            animate_next_configure: Cell::new(false),
            animation_snapshot: RefCell::new(None),
            rules: params.rules.unwrap_or_default(),
        }))
    }

    fn communicate(&self) -> bool {
        let mut changed = false;

        let size = self.0.forced_size.get().or(self.0.requested_size.get());
        if let Some(size) = size {
            assert!(size.w >= 0);
            assert!(size.h >= 0);

            let mut new_bbox = self.0.initial_bbox;
            if size.w != 0 {
                new_bbox.size.w = size.w;
            }
            if size.h != 0 {
                new_bbox.size.h = size.h;
            }

            if self.0.bbox.get() != new_bbox {
                self.0.bbox.set(new_bbox);
                changed = true;
            }
        }

        self.0.animate_next_configure.set(false);

        if self.0.sizing_mode.get() != self.0.pending_sizing_mode.get() {
            self.0.sizing_mode.set(self.0.pending_sizing_mode.get());
            changed = true;
        }

        if self.0.is_windowed_fullscreen.get() != self.0.is_pending_windowed_fullscreen.get() {
            self.0
                .is_windowed_fullscreen
                .set(self.0.is_pending_windowed_fullscreen.get());
            changed = true;
        }

        changed
    }
}

impl LayoutElement for TestWindow {
    type Id = usize;

    fn id(&self) -> &Self::Id {
        &self.0.id
    }

    fn size(&self) -> Size<i32, Logical> {
        self.0.bbox.get().size
    }

    fn buf_loc(&self) -> Point<i32, Logical> {
        (0, 0).into()
    }

    fn is_in_input_region(&self, point: Point<f64, Logical>) -> bool {
        // Whole surface accepts input (lets hit-tests exercise Input hits).
        let size = self.size().to_f64();
        0. <= point.x && point.x < size.w && 0. <= point.y && point.y < size.h
    }

    fn request_size(
        &mut self,
        size: Size<i32, Logical>,
        mode: SizingMode,
        _animate: bool,
        _transaction: Option<Transaction>,
    ) {
        if self.0.requested_size.get() != Some(size) {
            self.0.requested_size.set(Some(size));
            self.0.animate_next_configure.set(true);
        }

        self.0.pending_sizing_mode.set(mode);

        if mode.is_fullscreen() {
            self.0.is_pending_windowed_fullscreen.set(false);
        }
    }

    fn min_size(&self) -> Size<i32, Logical> {
        self.0.min_size
    }

    fn max_size(&self) -> Size<i32, Logical> {
        self.0.max_size
    }

    fn is_wl_surface(&self, _wl_surface: &WlSurface) -> bool {
        false
    }

    fn set_preferred_scale_transform(&self, _scale: output::Scale, _transform: Transform) {}

    fn has_ssd(&self) -> bool {
        false
    }

    fn output_enter(&self, _output: &Output) {}

    fn output_leave(&self, _output: &Output) {}

    fn set_offscreen_data(&self, _data: Option<OffscreenData>) {}

    fn set_activated(&mut self, active: bool) {
        self.0.pending_activated.set(active);
    }

    fn set_bounds(&self, _bounds: Size<i32, Logical>) {}

    fn is_ignoring_opacity_window_rule(&self) -> bool {
        false
    }

    fn configure_intent(&self) -> ConfigureIntent {
        ConfigureIntent::CanSend
    }

    fn send_pending_configure(&mut self) {}

    fn set_active_in_tile(&mut self, _active: bool) {}

    fn set_floating(&mut self, _floating: bool) {}

    fn sizing_mode(&self) -> SizingMode {
        self.0.sizing_mode.get()
    }

    fn pending_sizing_mode(&self) -> SizingMode {
        self.0.pending_sizing_mode.get()
    }

    fn requested_size(&self) -> Option<Size<i32, Logical>> {
        self.0.requested_size.get()
    }

    fn is_windowed_fullscreen(&self) -> bool {
        self.0.is_windowed_fullscreen.get()
    }

    fn is_pending_windowed_fullscreen(&self) -> bool {
        self.0.is_pending_windowed_fullscreen.get()
    }

    fn request_windowed_fullscreen(&mut self, value: bool) {
        self.0.is_pending_windowed_fullscreen.set(value);
    }

    fn is_child_of(&self, parent: &Self) -> bool {
        self.0.parent_id.get() == Some(parent.0.id)
    }

    fn refresh(&self) {}

    fn rules(&self) -> &ResolvedWindowRules {
        &self.0.rules
    }

    fn take_animation_snapshot(&mut self) -> Option<LayoutElementRenderSnapshot> {
        self.0.animation_snapshot.take()
    }

    fn set_interactive_resize(&mut self, _data: Option<InteractiveResizeData>) {}

    fn cancel_interactive_resize(&mut self) {}

    fn on_commit(&mut self, _serial: Serial) {}

    fn interactive_resize_data(&self) -> Option<InteractiveResizeData> {
        None
    }

    fn is_urgent(&self) -> bool {
        false
    }
}

fn test_options() -> Rc<Options> {
    Rc::new(Options::default())
}

fn test_space() -> (Clock, CanvasSpace<TestWindow>) {
    let clock = Clock::with_time(Duration::ZERO);
    let view_size = Size::from((1920., 1080.));
    let working_area = Rectangle::from_size(Size::from((1920., 1080.)));
    let space = CanvasSpace::new(view_size, working_area, 1., clock.clone(), test_options());
    (clock, space)
}

fn make_tile(space: &CanvasSpace<TestWindow>, id: usize) -> Tile<TestWindow> {
    Tile::new(
        TestWindow::new(TestWindowParams::new(id)),
        space.view_size(),
        1.,
        space.clock().clone(),
        space.options().clone(),
    )
}

/// Adds a window at an absolute canvas position, activated.
fn add_at(space: &mut CanvasSpace<TestWindow>, id: usize, x: f64, y: f64) {
    let tile = make_tile(space, id);
    space.add_tile(tile, Point::from((x, y)), true, None);
}

fn canvas_pos(space: &CanvasSpace<TestWindow>, id: usize) -> Point<f64, Logical> {
    space
        .tiles_with_canvas_positions()
        .find(|(tile, _)| *tile.window().id() == id)
        .map(|(_, pos)| pos)
        .unwrap()
}

fn active_id(space: &CanvasSpace<TestWindow>) -> usize {
    *space.active_window().unwrap().id()
}

#[test]
fn spawn_position_is_view_center() {
    let (_clock, space) = test_space();
    let tile = make_tile(&space, 1);
    let size = tile.tile_size();
    let pos = space.spawn_position(&tile);
    // View 1920x1080, camera at origin, zoom 1: visible origin is (-960, -540).
    assert_eq!(
        pos,
        Point::from((-960. + (1920. - size.w) / 2., -540. + (1080. - size.h) / 2.))
    );
}

#[test]
fn added_windows_keep_absolute_positions() {
    let (_clock, mut space) = test_space();
    add_at(&mut space, 1, -500., -200.);
    add_at(&mut space, 2, 300., 400.);
    add_at(&mut space, 3, 0., 0.);

    // No automatic rearrangement: every tile stays exactly where put.
    assert_eq!(canvas_pos(&space, 1), Point::from((-500., -200.)));
    assert_eq!(canvas_pos(&space, 2), Point::from((300., 400.)));
    assert_eq!(canvas_pos(&space, 3), Point::from((0., 0.)));
    space.verify_invariants();
}

#[test]
fn spatial_focus_left_right() {
    let (_clock, mut space) = test_space();
    add_at(&mut space, 1, -500., 0.);
    add_at(&mut space, 2, 0., 0.);
    add_at(&mut space, 3, 500., 0.);
    assert_eq!(active_id(&space), 3);

    assert!(space.focus_left());
    assert_eq!(active_id(&space), 2);
    assert!(space.focus_left());
    assert_eq!(active_id(&space), 1);
    // Nothing further left.
    assert!(!space.focus_left());
    assert_eq!(active_id(&space), 1);

    assert!(space.focus_right());
    assert_eq!(active_id(&space), 2);
    space.verify_invariants();
}

#[test]
fn spatial_focus_up_down() {
    let (_clock, mut space) = test_space();
    add_at(&mut space, 1, 0., -500.);
    add_at(&mut space, 2, 0., 0.);
    add_at(&mut space, 3, 0., 500.);
    assert_eq!(active_id(&space), 3);

    assert!(space.focus_up());
    assert_eq!(active_id(&space), 2);
    assert!(space.focus_up());
    assert_eq!(active_id(&space), 1);
    assert!(!space.focus_up());

    assert!(space.focus_down());
    assert_eq!(active_id(&space), 2);
    space.verify_invariants();
}

#[test]
fn focus_extremes() {
    let (_clock, mut space) = test_space();
    add_at(&mut space, 1, -500., 300.);
    add_at(&mut space, 2, 400., -300.);
    add_at(&mut space, 3, 0., 0.);

    space.focus_leftmost();
    assert_eq!(active_id(&space), 1);
    space.focus_rightmost();
    assert_eq!(active_id(&space), 2);
    space.focus_topmost();
    assert_eq!(active_id(&space), 2);
    space.focus_bottommost();
    assert_eq!(active_id(&space), 1);
    space.verify_invariants();
}

#[test]
fn keyboard_nudge_moves_tile() {
    let (_clock, mut space) = test_space();
    add_at(&mut space, 1, 100., 100.);
    assert_eq!(active_id(&space), 1);

    assert!(space.move_right());
    assert_eq!(canvas_pos(&space, 1), Point::from((150., 100.)));
    assert!(space.move_down());
    assert_eq!(canvas_pos(&space, 1), Point::from((150., 150.)));
    assert!(space.move_left());
    assert!(space.move_up());
    assert_eq!(canvas_pos(&space, 1), Point::from((100., 100.)));
    space.verify_invariants();
}

#[test]
fn activate_centers_camera_on_window() {
    let (_clock, mut space) = test_space();
    add_at(&mut space, 1, 0., 0.);
    add_at(&mut space, 2, 2000., 1500.);

    // Activating window 1 targets the camera at its center.
    space.activate_window(&1);
    let (tx, ty, tz) = space.canvas_viewport().target();
    assert_eq!(tz, 1.);
    // Tile 1 is 100x200 at (0, 0), so its center is (50, 100).
    assert_eq!((tx, ty), (50., 100.));
    space.verify_invariants();
}

#[test]
fn camera_springs_converge_after_activate() {
    let (mut clock, mut space) = test_space();
    add_at(&mut space, 1, 3000., 2000.);
    space.activate_window(&1);

    // March the frame clock forward; the camera must converge on the window.
    // Note: no clock.clear() here — that would re-fetch monotonic time instead
    // of the scripted test time.
    for step in 1..=60 {
        clock.set_unadjusted(Duration::from_millis(step * 100));
        space.advance_animations();
    }
    let vp = space.canvas_viewport();
    assert!((vp.cam_x - 3050.).abs() < 1., "cam_x = {}", vp.cam_x);
    assert!((vp.cam_y - 2100.).abs() < 1., "cam_y = {}", vp.cam_y);
    space.verify_invariants();
}

#[test]
fn remove_keeps_other_positions() {
    let (_clock, mut space) = test_space();
    add_at(&mut space, 1, -500., 0.);
    add_at(&mut space, 2, 0., 0.);
    add_at(&mut space, 3, 500., 0.);

    space.remove_tile(&2, Transaction::new());
    assert_eq!(canvas_pos(&space, 1), Point::from((-500., 0.)));
    assert_eq!(canvas_pos(&space, 3), Point::from((500., 0.)));
    // The active window (3) is untouched.
    assert_eq!(active_id(&space), 3);
    // Removing the active window falls back to the topmost remaining tile.
    space.remove_tile(&3, Transaction::new());
    assert_eq!(active_id(&space), 1);
    space.verify_invariants();
}

#[test]
fn set_window_width_and_height() {
    let (_clock, mut space) = test_space();
    add_at(&mut space, 1, 0., 0.);

    space.set_window_width(Some(&1), SizeChange::SetFixed(800));
    let tile = space.tiles().find(|t| *t.window().id() == 1).unwrap();
    assert_eq!(tile.window().requested_size(), Some(Size::from((800, 200))));

    space.set_window_height(Some(&1), SizeChange::SetFixed(600));
    let tile = space.tiles().find(|t| *t.window().id() == 1).unwrap();
    assert_eq!(tile.window().requested_size(), Some(Size::from((800, 600))));

    // Communicating the configure applies it; the space picks the new size
    // up on commit, like in the real compositor flow.
    tile.window().communicate();
    space.update_window(&1usize, None);
    let tile = space.tiles().find(|t| *t.window().id() == 1).unwrap();
    assert_eq!(tile.tile_size(), Size::from((800., 600.)));
    space.verify_invariants();
}

#[test]
fn toggle_full_width_roundtrip() {
    let (_clock, mut space) = test_space();
    add_at(&mut space, 1, 0., 0.);

    space.toggle_full_width();
    let tile = space.tiles().find(|t| *t.window().id() == 1).unwrap();
    // Working area is 1920 wide.
    assert_eq!(tile.window().requested_size().map(|s| s.w), Some(1920));

    space.toggle_full_width();
    let tile = space.tiles().find(|t| *t.window().id() == 1).unwrap();
    // Restored to the natural 100px width.
    assert_eq!(tile.window().requested_size().map(|s| s.w), Some(100));
    space.verify_invariants();
}

#[test]
fn fullscreen_and_restore() {
    let (_clock, mut space) = test_space();
    add_at(&mut space, 1, 0., 0.);

    assert!(space.set_fullscreen(&1, true));
    let tile = space.tiles().find(|t| *t.window().id() == 1).unwrap();
    assert!(tile.window().pending_sizing_mode().is_fullscreen());
    // No-op when already fullscreen.
    assert!(!space.set_fullscreen(&1, true));

    assert!(space.set_fullscreen(&1, false));
    let tile = space.tiles().find(|t| *t.window().id() == 1).unwrap();
    assert!(tile.window().pending_sizing_mode().is_normal());
    space.verify_invariants();
}

#[test]
fn maximized_and_restore() {
    let (_clock, mut space) = test_space();
    add_at(&mut space, 1, 0., 0.);

    assert!(space.set_maximized(&1, true));
    let tile = space.tiles().find(|t| *t.window().id() == 1).unwrap();
    assert!(tile.window().pending_sizing_mode().is_maximized());
    assert_eq!(
        tile.window().requested_size(),
        Some(Size::from((1920, 1080)))
    );

    assert!(space.set_maximized(&1, false));
    let tile = space.tiles().find(|t| *t.window().id() == 1).unwrap();
    assert!(tile.window().pending_sizing_mode().is_normal());
    space.verify_invariants();
}

#[test]
fn window_under_reports_output_space_origin_and_scale() {
    // Pointer mapping contract: at any camera zoom, window_under() must
    // report the surface origin in output (view) coordinates plus the zoom,
    // so that (pos - win_pos) / scale recovers exact surface-local units.
    let (_clock, mut space) = test_space();
    add_at(&mut space, 1, 0., 0.);

    // Zoom out to 0.5 around the screen center and settle.
    space
        .canvas_viewport_mut()
        .zoom_at_screen(0.5, (960., 540.));
    space.canvas_viewport_mut().snap_to_targets();

    // Tile is 100x200 at canvas (0,0); view origin of the tile:
    let (ox, oy) = space.canvas_viewport().canvas_to_screen(0., 0.);

    // Query the tile middle in view coordinates.
    let query = Point::from((ox + 25., oy + 50.));
    let (_, hit) = space.window_under(query).expect("tile must hit");
    match hit {
        HitType::Input { win_pos, scale } => {
            assert!((scale - 0.5).abs() < 1e-9);
            assert_eq!(win_pos, Point::from((ox, oy)));
            // Full inversion lands on the tile middle in surface units.
            let local = (query - win_pos).downscale(scale);
            assert_eq!(local, Point::from((50., 100.)));
        }
        _ => panic!("expected input hit, got {hit:?}"),
    }
    space.verify_invariants();
}

#[test]
fn window_under_with_borders_far_camera_and_zoom() {
    // Live-session reproduction: 2px borders on, camera far from the origin,
    // zoomed out. The reported origin must include the border offset scaled
    // by the zoom, and inversion must land exactly in surface units.
    use nyx_config::Layout as ConfigLayout;

    let mut config_layout = ConfigLayout::default();
    config_layout.border.off = false;
    config_layout.border.width = 2.;
    let options = Rc::new(Options {
        layout: config_layout,
        ..Options::default()
    });

    let clock = Clock::with_time(Duration::ZERO);
    let view_size = Size::from((1920., 1080.));
    let working_area = Rectangle::from_size(Size::from((1920., 1080.)));
    let mut space = CanvasSpace::new(view_size, working_area, 1., clock, options);

    // Tile 100x200 window + 2px borders = 104x204 tile at a far position.
    let tile = Tile::new(
        TestWindow::new(TestWindowParams::new(1)),
        space.view_size(),
        1.,
        space.clock().clone(),
        space.options().clone(),
    );
    space.add_tile(tile, Point::from((-5000., -3000.)), true, None);

    // Pan far away and zoom out, then settle.
    space.canvas_viewport_mut().pan_by(-4000., -2000.);
    space
        .canvas_viewport_mut()
        .zoom_at_screen(0.404, (960., 540.));
    space.canvas_viewport_mut().snap_to_targets();

    let zoom = space.canvas_viewport().zoom;
    assert!((zoom - 0.404).abs() < 1e-9);

    let tile = space.tiles().find(|t| *t.window().id() == 1).unwrap();
    assert_eq!(tile.tile_size(), Size::from((104., 204.)));
    let buf = tile.buf_loc();

    // View origin of the tile and a query at the window middle.
    let (ox, oy) = space.canvas_viewport().canvas_to_screen(-5000., -3000.);
    // Window middle in view coords: tile origin + (border + half) * zoom.
    let query = Point::from((ox + (2. + 50.) * zoom, oy + (2. + 100.) * zoom));

    let (_, hit) = space.window_under(query).expect("tile must hit");
    match hit {
        HitType::Input { win_pos, scale } => {
            assert!((scale - zoom).abs() < 1e-9);
            let expected = Point::from((ox, oy)) + buf.upscale(zoom);
            assert_eq!(win_pos, expected);
            // Full inversion lands on the window middle in surface units.
            let local = (query - win_pos).downscale(scale);
            assert!((local.x - 50.).abs() < 1e-9);
            assert!((local.y - 100.).abs() < 1e-9);
        }
        _ => panic!("expected input hit, got {hit:?}"),
    }
    space.verify_invariants();
}

#[test]
fn unfullscreen_without_restore_size_still_clears_state() {
    // A window that opens already fullscreen has no remembered restore size;
    // exiting must still clear the state (send a configure), never stick.
    let (_clock, mut space) = test_space();
    let mut tile = make_tile(&space, 1);
    tile.window_mut().request_size(
        Size::from((1920, 1080)),
        SizingMode::Fullscreen,
        false,
        None,
    );
    space.add_tile(tile, Point::from((0., 0.)), true, None);
    assert!(space.fullscreen_lock());

    assert!(space.set_fullscreen(&1, false));
    let tile = space.tiles().find(|t| *t.window().id() == 1).unwrap();
    assert!(tile.window().pending_sizing_mode().is_normal());
    assert!(!space.fullscreen_lock());
    space.verify_invariants();
}

#[test]
fn camera_springs_freeze_while_deferred() {
    // Mid-flight clicks must map coherently: while frozen (pointer buttons
    // held, no grab), camera springs hold live values even as time passes;
    // on release they resume toward retargeted goals.
    let (mut clock, mut space) = test_space();
    space.canvas_viewport_mut().pan_by(2000., 2000.);

    // Let time pass unfrozen, frame by frame: camera advances toward goal.
    for t in 1..=30 {
        clock.set_unadjusted(Duration::from_millis(t * 16));
        space.advance_animations();
    }
    let (mx, my, _) = space.canvas_viewport().target();
    let (cx, cy) = (space.canvas_viewport().cam_x, space.canvas_viewport().cam_y);
    assert!(cx > 0. && cy > 0., "camera should advance unfrozen");

    // Freeze mid-flight: time passes, live values hold.
    space.set_camera_frozen(true);
    for t in 31..=60 {
        clock.set_unadjusted(Duration::from_millis(t * 16));
        space.advance_animations();
    }
    assert_eq!(space.canvas_viewport().cam_x, cx);
    assert_eq!(space.canvas_viewport().cam_y, cy);
    // Targets are untouched by the freeze.
    assert_eq!(space.canvas_viewport().target(), (mx, my, 1.));

    // Release: springs resume toward the goal.
    space.set_camera_frozen(false);
    for t in 61..=90 {
        clock.set_unadjusted(Duration::from_millis(t * 16));
        space.advance_animations();
    }
    assert!(space.canvas_viewport().cam_x > cx);
    space.verify_invariants();
}

#[test]
fn zoom_pivot_uses_live_camera_mid_flight() {
    // Zooming again mid-flight must pivot around what is currently displayed
    // (live camera), not where the springs are heading (targets). Off-center
    // pivot so the camera actually travels.
    let (mut clock, mut space) = test_space();
    space
        .canvas_viewport_mut()
        .zoom_at_screen(0.5, (1200., 700.));
    for t in 1..=3 {
        clock.set_unadjusted(Duration::from_millis(t * 16));
        space.advance_animations();
    }
    let before = space.canvas_viewport().screen_to_canvas(1200., 700.);
    space
        .canvas_viewport_mut()
        .zoom_at_screen(0.3, (1200., 700.));
    space.canvas_viewport_mut().snap_to_targets();
    let after = space.canvas_viewport().screen_to_canvas(1200., 700.);
    assert!(
        (before.0 - after.0).abs() < 1e-6 && (before.1 - after.1).abs() < 1e-6,
        "pivot point drifted mid-flight: {before:?} vs {after:?}"
    );
    space.verify_invariants();
}

#[test]
fn window_under_respects_camera() {
    let (_clock, mut space) = test_space();
    add_at(&mut space, 1, 0., 0.);

    // Tile is 100x200 at canvas (0, 0); at 1:1 with the camera at the origin
    // it spans view (910..1010, 440..640).
    let vp = space.canvas_viewport();
    let (sx, sy) = vp.canvas_to_screen(50., 100.);
    assert_eq!((sx, sy), (1010., 640.));

    // Hit the tile middle in view coordinates.
    let hit = space.window_under(Point::from((960. + 50., 540. + 100.)));
    assert!(hit.is_some());
    assert_eq!(*hit.unwrap().0.id(), 1);
    // Far away misses.
    assert!(space.window_under(Point::from((0., 0.))).is_none());
    space.verify_invariants();
}

#[test]
fn scroll_amount_zero_when_visible() {
    let (_clock, mut space) = test_space();
    add_at(&mut space, 1, 0., 0.);
    // Freshly spawned at view center: fully visible, nothing to pan.
    assert_eq!(space.scroll_amount_to_activate(&1), 0.);

    // Far-away, inactive window requires panning.
    add_at(&mut space, 2, 10000., 10000.);
    space.activate_window(&1);
    assert!(space.scroll_amount_to_activate(&2) > 0.);
    space.verify_invariants();
}

#[test]
fn layout_add_window_spawns_at_view_center() {
    let mut layout = Layout::<TestWindow>::default();
    let window = TestWindow::new(TestWindowParams::new(1));
    layout.add_window(
        window,
        AddWindowTarget::Auto,
        None,
        None,
        false,
        false,
        ActivateWindow::Yes,
    );

    // NoOutputs workspace is 1280x720; tile 100x200 lands centered.
    let (_, pos, _) = layout
        .workspaces()
        .next()
        .unwrap()
        .2
        .tiles_with_render_positions()
        .next()
        .unwrap();
    assert_eq!(pos, Point::from(((1280. - 100.) / 2., (720. - 200.) / 2.)));
    layout.verify_invariants();
}

#[test]
fn open_and_close_animation_lifecycle() {
    let (_clock, mut space) = test_space();
    add_at(&mut space, 1, 0., 0.);

    assert!(space.start_open_animation(&1));
    assert!(!space.start_open_animation(&999));
    space.verify_invariants();
}

#[test]
fn dragged_window_stays_pinned_to_cursor_when_zoomed() {
    // Regression test: a grabbed window must not jump away from the cursor
    // when the camera zoom is not 1:1. The grab offset is stored in canvas
    // units, so tile_render_location() must scale it by the camera zoom.
    use smithay::output::{Output, PhysicalProperties, Subpixel};

    let (_clock, space) = test_space();
    let tile = make_tile(&space, 1);

    let output = Output::new(
        "test".to_owned(),
        PhysicalProperties {
            size: (0, 0).into(),
            subpixel: Subpixel::Unknown,
            make: String::new(),
            model: String::new(),
            serial_number: String::new(),
        },
    );

    // Window is 100x200 canvas units, grabbed at its center.
    let move_ = InteractiveMoveData {
        tile,
        output,
        pointer_pos_within_output: Point::from((960., 540.)),
        grab_pos: Point::from((0., 0.)),
        is_floating: false,
        canvas_zoom: 0.5,
        pointer_ratio_within_window: (0.5, 0.5),
        output_config: None,
        workspace_config: None,
    };

    // View-space grab offset is (50, 100) canvas units = (25, 50) output
    // pixels at 0.5 zoom, so the tile must render at (935, 490).
    let pos = move_.tile_render_location(1.);
    assert_eq!(pos, Point::from((935., 490.)));
}

#[test]
fn interactive_resize_survives_multiple_updates() {
    // Regression test: pointer resize must keep working across frames (it
    // used to cancel itself after the first update) and honor camera zoom.
    let (_clock, mut space) = test_space();
    add_at(&mut space, 1, 0., 0.);

    assert!(space.interactive_resize_begin(1, ResizeEdge::RIGHT));
    assert!(space.interactive_resize_update(&1, Point::from((10., 0.))));
    let tile = space.tiles().find(|t| *t.window().id() == 1).unwrap();
    assert_eq!(tile.window().requested_size().unwrap().w, 110);

    // Second update must still apply (old code returned false here).
    assert!(space.interactive_resize_update(&1, Point::from((30., 0.))));
    let tile = space.tiles().find(|t| *t.window().id() == 1).unwrap();
    assert_eq!(tile.window().requested_size().unwrap().w, 130);
    space.verify_invariants();
}

#[test]
fn interactive_resize_compensates_camera_zoom() {
    // At 0.5 zoom, a 10px view-space drag resizes by 20 canvas units.
    let (_clock, mut space) = test_space();
    add_at(&mut space, 1, 0., 0.);

    space
        .canvas_viewport_mut()
        .zoom_at_screen(0.5, (960., 540.));
    space.canvas_viewport_mut().snap_to_targets();

    assert!(space.interactive_resize_begin(1, ResizeEdge::RIGHT));
    assert!(space.interactive_resize_update(&1, Point::from((10., 0.))));
    let tile = space.tiles().find(|t| *t.window().id() == 1).unwrap();
    assert_eq!(tile.window().requested_size().unwrap().w, 120);
    space.verify_invariants();
}

#[test]
fn fullscreen_freezes_camera_and_saves_view() {
    // Exclusive fullscreen must not fly the camera: it renders in screen
    // space while the camera stays frozen, and the pre-fullscreen view is
    // saved for restore on exit.
    let (_clock, mut space) = test_space();
    add_at(&mut space, 1, 2000., 1500.);

    space.canvas_viewport_mut().pan_by(3000., 2000.);
    space
        .canvas_viewport_mut()
        .zoom_at_screen(0.5, (960., 540.));
    space.canvas_viewport_mut().snap_to_targets();
    let before = space.canvas_viewport().target();

    assert!(space.set_fullscreen(&1, true));
    // Camera untouched (no centering flight, no zoom reset).
    assert_eq!(space.canvas_viewport().target(), before);
    // Navigation lock engages immediately (pending fullscreen).
    assert!(space.fullscreen_lock());
    // Screen-space tile only after the client acks (committed).
    assert!(space.exclusive_fullscreen_tile().is_none());

    // Ack the configure: committed fullscreen, exclusive tile available.
    let tile = space.tiles_mut().find(|t| *t.window().id() == 1).unwrap();
    tile.window().communicate();
    space.update_window(&1, None);
    assert!(space.exclusive_fullscreen_tile().is_some());

    // Exit unlocks and restores the saved view.
    assert!(space.set_fullscreen(&1, false));
    assert!(!space.fullscreen_lock());
    assert_eq!(space.canvas_viewport().target(), before);
    space.verify_invariants();
}

#[test]
fn fullscreen_lock_blocks_camera_moves() {
    let (_clock, mut space) = test_space();
    add_at(&mut space, 1, 0., 0.);
    // add_at with activate centers the camera on the 100x200 tile at (0,0).
    let before = space.canvas_viewport().target();
    assert_eq!(before, (50., 100., 1.));

    assert!(space.set_fullscreen(&1, true));
    assert!(space.fullscreen_lock());

    // Focus-tracking auto-centering is frozen while locked.
    space.center_on_window(&1);
    assert_eq!(space.canvas_viewport().target(), before);

    assert!(space.set_fullscreen(&1, false));
    assert!(!space.fullscreen_lock());
    // Unlocked: centering works again after moving the camera away.
    space.canvas_viewport_mut().pan_by(500., 500.);
    space.center_on_window(&1);
    assert_eq!(space.canvas_viewport().target(), (50., 100., 1.));
    space.verify_invariants();
}
