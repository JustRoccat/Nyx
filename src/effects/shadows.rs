#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ShadowParams {
    /// Blur radius (sigma-ish) in logical pixels.
    pub radius: f64,
    /// Offset (x, y) in logical pixels.
    pub offset_x: f64,
    pub offset_y: f64,
    /// Opacity 0..1.
    pub opacity: f64,
    /// Corner radius to follow (canvas units; scaled by zoom at render).
    pub corner_radius: f64,
    pub enabled: bool,
    pub render_power: u8,
    pub scale: f64,
}

impl ShadowParams {
    pub fn new(radius: f64, offset_x: f64, offset_y: f64, opacity: f64) -> Self {
        Self {
            radius: radius.max(0.0),
            offset_x,
            offset_y,
            opacity: opacity.clamp(0.0, 1.0),
            corner_radius: 0.0,
            enabled: true,
            render_power: 3,
            scale: 1.0,
        }
    }

    pub fn from_config(shadow: nyx_config::Shadow) -> Self {
        Self {
            radius: shadow.softness.max(0.0),
            offset_x: shadow.offset.x.0,
            offset_y: shadow.offset.y.0,
            opacity: shadow.color.a.clamp(0.0, 1.0) as f64,
            corner_radius: 0.0,
            enabled: shadow.on,
            render_power: shadow.render_power.clamp(1, 4),
            scale: shadow.scale.clamp(0.1, 4.0),
        }
    }

    pub fn radius_for_zoom(&self, zoom: f64) -> f64 {
        let base = self.radius * zoom.max(1e-6) * self.scale.clamp(0.1, 4.0);
        let extra = 1.0 + (f64::from(self.render_power.saturating_sub(1))) * 0.25;
        base * extra
    }

    pub fn sigma_for_range(range: f64) -> f64 {
        (range.max(0.0)) / 2.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SoftShadow {
    pub params: ShadowParams,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl SoftShadow {
    pub fn for_window(
        params: ShadowParams,
        win_x: f64,
        win_y: f64,
        win_w: f64,
        win_h: f64,
        zoom: f64,
    ) -> Self {
        let r = params.radius_for_zoom(zoom);
        Self {
            params,
            x: win_x + params.offset_x * zoom - r * 2.0,
            y: win_y + params.offset_y * zoom - r * 2.0,
            w: win_w + r * 4.0,
            h: win_h + r * 4.0,
        }
    }

    /// Approximate Gaussian falloff at distance `d` (0 at center).
    pub fn falloff(&self, d: f64) -> f64 {
        let sigma = (self.params.radius.max(1.0)) / 2.0;
        (-d * d / (2.0 * sigma * sigma)).exp() * self.params.opacity
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn falloff_decreases() {
        let p = ShadowParams::new(12.0, 0.0, 4.0, 0.5);
        let s = SoftShadow::for_window(p, 0.0, 0.0, 100.0, 100.0, 1.0);
        assert!(s.falloff(0.0) > s.falloff(10.0));
    }

    #[test]
    fn render_power_softens() {
        let mut p = ShadowParams::new(12.0, 0.0, 0.0, 0.5);
        let base = p.radius_for_zoom(1.0);
        p.render_power = 4;
        assert!(p.radius_for_zoom(1.0) > base);
    }

    #[test]
    fn sigma_convention() {
        assert!((ShadowParams::sigma_for_range(30.0) - 15.0).abs() < 1e-9);
    }
}
