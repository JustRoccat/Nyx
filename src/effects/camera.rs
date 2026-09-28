pub fn slidefade(progress: f64, fraction: f64) -> (f64, f64) {
    let p = progress.clamp(0.0, 1.0);
    let f = fraction.clamp(0.0, 1.0);
    ((1.0 - p) * f, p)
}

pub fn slide_offset(side: SlideSide, progress: f64, distance: f64) -> (f64, f64) {
    let p = (1.0 - progress.clamp(0.0, 1.0)) * distance;
    match side {
        SlideSide::Left => (-p, 0.0),
        SlideSide::Right => (p, 0.0),
        SlideSide::Top => (0.0, -p),
        SlideSide::Bottom => (0.0, p),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlideSide {
    Left,
    Right,
    Top,
    Bottom,
}

pub fn popin_scale(progress: f64, min_scale: f64, closing: bool) -> f64 {
    let p = progress.clamp(0.0, 1.0);
    let m = min_scale.clamp(0.0, 1.0);
    if closing { 1.0 - (1.0 - m) * p } else { m + (1.0 - m) * p }
}

pub fn window_slide_offset(
    dir: nyx_config::animations::SlideDirection,
    travel: f64,
    w: f64,
    h: f64,
) -> (f64, f64) {
    use nyx_config::animations::SlideDirection::*;
    const FRACTION: f64 = 0.3;
    let t = travel.clamp(0.0, 1.0);
    match dir {
        Left => (-t * FRACTION * w, 0.0),
        Right => (t * FRACTION * w, 0.0),
        Top => (0.0, -t * FRACTION * h),
        Bottom => (0.0, t * FRACTION * h),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slidefade_endpoints() {
        let (off, fade) = slidefade(0.0, 0.2);
        assert!((off - 0.2).abs() < 1e-9);
        assert_eq!(fade, 0.0);
        let (off, fade) = slidefade(1.0, 0.2);
        assert_eq!(off, 0.0);
        assert_eq!(fade, 1.0);
    }

    #[test]
    fn slide_sides_oppose() {
        let (lx, _) = slide_offset(SlideSide::Left, 0.0, 100.0);
        let (rx, _) = slide_offset(SlideSide::Right, 0.0, 100.0);
        assert!((lx + rx).abs() < 1e-9);
    }

    #[test]
    fn popin_open_and_close_mirror() {
        assert!((popin_scale(0.0, 0.87, false) - 0.87).abs() < 1e-9);
        assert!((popin_scale(1.0, 0.87, false) - 1.0).abs() < 1e-9);
        assert!((popin_scale(1.0, 0.87, true) - 0.87).abs() < 1e-9);
    }

    #[test]
    fn window_slide_moves_toward_direction() {
        use nyx_config::animations::SlideDirection::*;
        let (x, y) = window_slide_offset(Left, 1.0, 100.0, 50.0);
        assert!((x + 30.0).abs() < 1e-9);
        assert_eq!(y, 0.0);
        let (x, y) = window_slide_offset(Bottom, 0.5, 100.0, 50.0);
        assert_eq!(x, 0.0);
        assert!((y - 7.5).abs() < 1e-9);
        let (x, y) = window_slide_offset(Right, 0.0, 100.0, 50.0);
        assert_eq!((x, y), (0.0, 0.0));
    }
}
