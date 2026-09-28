#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CornerRadii {
    pub top_left: f64,
    pub top_right: f64,
    pub bottom_right: f64,
    pub bottom_left: f64,
}

impl CornerRadii {
    pub fn uniform(r: f64) -> Self {
        Self {
            top_left: r,
            top_right: r,
            bottom_right: r,
            bottom_left: r,
        }
    }

    pub fn for_zoom(&self, zoom: f64) -> Self {
        let z = zoom.max(1e-6);
        Self {
            top_left: self.top_left * z,
            top_right: self.top_right * z,
            bottom_right: self.bottom_right * z,
            bottom_left: self.bottom_left * z,
        }
    }
}

pub fn corner_radius_for_zoom(config_radius: f64, zoom: f64) -> f64 {
    config_radius * zoom.max(1e-6)
}

pub fn rounding_power_scale(power: f64) -> f64 {
    let p = power.clamp(1.0, 4.0);
    0.85 + p * 0.075
}

pub fn corner_radius_with_power(radius: f64, power: f64, zoom: f64) -> f64 {
    let r = radius.max(0.0);
    if r == 0.0 {
        return 0.0;
    }
    corner_radius_for_zoom(r, zoom) * rounding_power_scale(power)
}

/// SDF of a rounded box, in shader units.
///
/// `p` is the fragment position relative to the rect center, `b` is half-size,
/// `r` the corner radius. Returns signed distance (negative inside).
/// Mirrors the GLSL we inject into the Smithay surface shader.
pub fn sd_rounded_box(px: f64, py: f64, bx: f64, by: f64, r: f64) -> f64 {
    let qx = px.abs() - bx + r;
    let qy = py.abs() - by + r;
    let ax = qx.max(0.0);
    let ay = qy.max(0.0);
    (ax * ax + ay * ay).sqrt() + qx.min(0.0).max(qy.min(0.0)) - r
}

/// Smoothstep antialiased coverage in `[0, 1]`.
pub fn smooth_coverage(sdf: f64, aa_width: f64) -> f64 {
    let w = aa_width.max(1e-6);
    let t = ((-sdf / w) + 0.5).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Reference GLSL snippet (documentation; real shader lives in render_helpers).
pub const ROUNDED_CORNERS_GLSL: &str = r#"
float sdRoundedBox(vec2 p, vec2 b, float r) {
    vec2 q = abs(p) - b + r;
    return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r;
}
float cornerCoverage(vec2 p, vec2 halfSize, float radius) {
    float d = sdRoundedBox(p, halfSize, radius);
    float aa = fwidth(d) * 1.2 + 1e-4;
    return 1.0 - smoothstep(-aa, aa, d);
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn center_is_inside() {
        assert!(sd_rounded_box(0.0, 0.0, 100.0, 100.0, 12.0) < 0.0);
    }

    #[test]
    fn far_outside_is_positive() {
        assert!(sd_rounded_box(500.0, 500.0, 100.0, 100.0, 12.0) > 0.0);
    }

    #[test]
    fn zoom_scales_radius() {
        assert!((corner_radius_for_zoom(12.0, 0.5) - 6.0).abs() < 1e-9);
    }

    #[test]
    fn power_default_is_neutral() {
        assert!((rounding_power_scale(2.0) - 1.0).abs() < 1e-9);
        assert!(rounding_power_scale(1.0) < rounding_power_scale(4.0));
    }

    #[test]
    fn zero_radius_stays_zero_with_power() {
        assert_eq!(corner_radius_with_power(0.0, 4.0, 2.0), 0.0);
    }
}
