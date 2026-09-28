use super::viewport::Viewport;

/// Smallest tile we will ever ask for, in canvas units.
const MIN_W: f64 = 100.0;
const MIN_H: f64 = 80.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TilingKind {
    #[default]
    Off,

    MasterStack,

    Grid,

    Dwindle,
}

impl TilingKind {
    pub fn is_on(self) -> bool {
        !matches!(self, TilingKind::Off)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TilingRegion {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl TilingRegion {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self {
            x,
            y,
            w: w.max(MIN_W),
            h: h.max(MIN_H),
        }
    }

    pub fn from_viewport_target(viewport: &Viewport) -> Self {
        let (x, y, w, h) = viewport.target_visible_rect();
        Self::new(x, y, w, h)
    }

    pub fn contains(&self, px: f64, py: f64) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DynamicTiling {
    pub kind: TilingKind,

    pub gap: f64,

    pub master_ratio: f64,
}

impl Default for DynamicTiling {
    fn default() -> Self {
        Self {
            kind: TilingKind::Off,
            gap: 16.0,
            master_ratio: 0.6,
        }
    }
}

impl DynamicTiling {
    pub fn new(kind: TilingKind, gap: f64, master_ratio: f64) -> Self {
        Self {
            kind,
            gap: gap.max(0.0),
            master_ratio: master_ratio.clamp(0.2, 0.8),
        }
    }

    pub fn is_on(&self) -> bool {
        self.kind.is_on()
    }

    pub fn place_in(
        &self,
        region: TilingRegion,
        index: usize,
        count: usize,
    ) -> (f64, f64, f64, f64) {
        let count = count.max(1);
        let index = index.min(count - 1);

        let g = self.gap.max(0.0);

        let ix = region.x + g;
        let iy = region.y + g;
        let iw = (region.w - 2.0 * g).max(MIN_W);
        let ih = (region.h - 2.0 * g).max(MIN_H);

        let rect = match self.kind {
            TilingKind::Off => (ix, iy, iw, ih),
            TilingKind::MasterStack => {
                master_stack(ix, iy, iw, ih, g, self.master_ratio, index, count)
            }
            TilingKind::Grid => grid(ix, iy, iw, ih, g, index, count),
            TilingKind::Dwindle => dwindle(ix, iy, iw, ih, g, index, count),
        };

        (rect.0, rect.1, rect.2.max(MIN_W), rect.3.max(MIN_H))
    }

    pub fn place(
        &self,
        viewport: &Viewport,
        index: usize,
        count: usize,
        default_w: f64,
        default_h: f64,
    ) -> (f64, f64, f64, f64) {
        if !self.kind.is_on() {
            let (x, y) = viewport.spawn_center(default_w, default_h);
            return (x, y, default_w, default_h);
        }
        self.place_in(TilingRegion::from_viewport_target(viewport), index, count)
    }
}

fn master_stack(
    ix: f64,
    iy: f64,
    iw: f64,
    ih: f64,
    g: f64,
    master_ratio: f64,
    index: usize,
    count: usize,
) -> (f64, f64, f64, f64) {
    if count == 1 {
        return (ix, iy, iw, ih);
    }

    let master_w = ((iw - g) * master_ratio).max(MIN_W);
    if index == 0 {
        return (ix, iy, master_w, ih);
    }

    let stack_n = (count - 1) as f64;
    let stack_x = ix + master_w + g;
    let stack_w = (iw - master_w - g).max(MIN_W);
    let each_h = ((ih - (stack_n - 1.0) * g) / stack_n).max(MIN_H);
    let y = iy + (index - 1) as f64 * (each_h + g);
    (stack_x, y, stack_w, each_h)
}

fn grid(
    ix: f64,
    iy: f64,
    iw: f64,
    ih: f64,
    g: f64,
    index: usize,
    count: usize,
) -> (f64, f64, f64, f64) {
    let cols = ((count as f64).sqrt().ceil() as usize).max(1);
    let rows = count.div_ceil(cols).max(1);

    let col = index % cols;
    let row = index / cols;

    let w = ((iw - (cols - 1) as f64 * g) / cols as f64).max(MIN_W);
    let h = ((ih - (rows - 1) as f64 * g) / rows as f64).max(MIN_H);

    // Windows on the last, possibly incomplete row stretch to fill it.
    let in_last_row = row + 1 == rows;
    let last_row_n = count - row * cols;
    let (w, x) = if in_last_row && last_row_n < cols && last_row_n > 0 {
        let w = ((iw - (last_row_n - 1) as f64 * g) / last_row_n as f64).max(MIN_W);
        (w, ix + col as f64 * (w + g))
    } else {
        (w, ix + col as f64 * (w + g))
    };

    (x, iy + row as f64 * (h + g), w, h)
}

fn dwindle(
    ix: f64,
    iy: f64,
    iw: f64,
    ih: f64,
    g: f64,
    index: usize,
    count: usize,
) -> (f64, f64, f64, f64) {
    let mut x = ix;
    let mut y = iy;
    let mut w = iw;
    let mut h = ih;

    for _ in 0..index {
        if w >= h {
            let half = ((w - g) / 2.0).max(MIN_W);
            x += half + g;
            w = (w - half - g).max(MIN_W);
        } else {
            let half = ((h - g) / 2.0).max(MIN_H);
            y += half + g;
            h = (h - half - g).max(MIN_H);
        }
    }

    // Everything but the last window takes the first half of its sub-region.
    if index + 1 < count {
        if w >= h {
            w = ((w - g) / 2.0).max(MIN_W);
        } else {
            h = ((h - g) / 2.0).max(MIN_H);
        }
    }

    (x, y, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KINDS: [TilingKind; 3] = [
        TilingKind::MasterStack,
        TilingKind::Grid,
        TilingKind::Dwindle,
    ];

    fn region() -> TilingRegion {
        TilingRegion::new(-960.0, -540.0, 1920.0, 1080.0)
    }

    fn assert_inside(r: TilingRegion, rect: (f64, f64, f64, f64)) {
        let (x, y, w, h) = rect;
        assert!(x >= r.x - 1e-6, "{x} < {}", r.x);
        assert!(y >= r.y - 1e-6, "{y} < {}", r.y);
        assert!(x + w <= r.x + r.w + 1e-6);
        assert!(y + h <= r.y + r.h + 1e-6);
        assert!(w > 0.0 && h > 0.0);
    }

    #[test]
    fn all_layouts_stay_inside_the_region() {
        let r = region();
        for kind in KINDS {
            let t = DynamicTiling::new(kind, 16.0, 0.6);
            for count in 1..=8 {
                for index in 0..count {
                    assert_inside(r, t.place_in(r, index, count));
                }
            }
        }
    }

    #[test]
    fn single_window_fills_the_region() {
        let r = region();
        for kind in KINDS {
            let t = DynamicTiling::new(kind, 0.0, 0.6);
            let (x, y, w, h) = t.place_in(r, 0, 1);
            assert!((x - r.x).abs() < 1e-6);
            assert!((y - r.y).abs() < 1e-6);
            assert!((w - r.w).abs() < 1e-6);
            assert!((h - r.h).abs() < 1e-6);
        }
    }

    #[test]
    fn master_stack_splits_by_ratio() {
        let r = region();
        let t = DynamicTiling::new(TilingKind::MasterStack, 0.0, 0.6);
        let (_, _, master_w, master_h) = t.place_in(r, 0, 3);
        assert!((master_w - r.w * 0.6).abs() < 1e-6);
        assert!((master_h - r.h).abs() < 1e-6);

        let (sx, _, sw, sh) = t.place_in(r, 1, 3);
        assert!((sx - (r.x + master_w)).abs() < 1e-6);
        assert!((sw - r.w * 0.4).abs() < 1e-6);
        assert!((sh - r.h / 2.0).abs() < 1e-6);
    }

    #[test]
    fn tiles_do_not_overlap() {
        let r = region();
        for kind in KINDS {
            let t = DynamicTiling::new(kind, 8.0, 0.6);
            for count in 2..=6 {
                let rects: Vec<_> = (0..count).map(|i| t.place_in(r, i, count)).collect();
                for (i, a) in rects.iter().enumerate() {
                    for b in rects.iter().skip(i + 1) {
                        let overlap_x = a.0 < b.0 + b.2 - 1e-6 && b.0 < a.0 + a.2 - 1e-6;
                        let overlap_y = a.1 < b.1 + b.3 - 1e-6 && b.1 < a.1 + a.3 - 1e-6;
                        assert!(
                            !(overlap_x && overlap_y),
                            "{kind:?} {count}: {a:?} vs {b:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn off_falls_back_to_spawn_center() {
        let vp = Viewport::default();
        let t = DynamicTiling::default();
        let (x, y, w, h) = t.place(&vp, 0, 3, 800.0, 600.0);
        let (sx, sy) = vp.spawn_center(800.0, 600.0);
        assert!((x - sx).abs() < 1e-9 && (y - sy).abs() < 1e-9);
        assert!((w - 800.0).abs() < 1e-9 && (h - 600.0).abs() < 1e-9);
    }

    #[test]
    fn grid_places_inside_view() {
        let vp = Viewport::default();
        let t = DynamicTiling::new(TilingKind::Grid, 16.0, 0.6);
        let r = TilingRegion::from_viewport_target(&vp);
        for i in 0..4 {
            assert_inside(r, t.place(&vp, i, 4, 800.0, 600.0));
        }
    }
}
