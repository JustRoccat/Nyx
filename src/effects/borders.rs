#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BorderStyle {
    #[default]
    Solid,
    Linear,
    Angular,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rgba(pub [f64; 4]);

impl Rgba {
    pub fn mix(&self, other: &Self, t: f64) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self([
            self.0[0] + (other.0[0] - self.0[0]) * t,
            self.0[1] + (other.0[1] - self.0[1]) * t,
            self.0[2] + (other.0[2] - self.0[2]) * t,
            self.0[3] + (other.0[3] - self.0[3]) * t,
        ])
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AnimatedGradientBorder {
    pub style: BorderStyle,
    pub active_from: Rgba,
    pub active_to: Rgba,
    pub inactive_from: Rgba,
    pub inactive_to: Rgba,
    pub width: f64,
    pub angle_speed: f64,
    pub angle: f64,
    pub loop_animations: bool,
    pub allow_constant_repaint: bool,
}

impl Default for AnimatedGradientBorder {
    fn default() -> Self {
        Self {
            style: BorderStyle::Solid,
            active_from: Rgba([0.4, 0.6, 1.0, 1.0]),
            active_to: Rgba([0.6, 0.4, 1.0, 1.0]),
            inactive_from: Rgba([0.3, 0.3, 0.35, 1.0]),
            inactive_to: Rgba([0.3, 0.3, 0.35, 1.0]),
            width: 2.0,
            angle_speed: 0.6,
            angle: 0.0,
            loop_animations: false,
            allow_constant_repaint: false,
        }
    }
}

impl AnimatedGradientBorder {
    pub fn advance(&mut self, dt_secs: f64) {
        if self.style == BorderStyle::Angular {
            self.angle = (self.angle + self.angle_speed * dt_secs) % (2.0 * std::f64::consts::PI);
        }
    }

    pub fn needs_repaint(&self) -> bool {
        self.style == BorderStyle::Angular
            && self.loop_animations
            && self.allow_constant_repaint
    }

    pub fn apply_angle_config(&mut self, angle_speed: f64, looping: bool, allow_repaint: bool) {
        self.angle_speed = angle_speed.max(0.0);
        self.loop_animations = looping;
        self.allow_constant_repaint = allow_repaint;
        if looping {
            self.style = BorderStyle::Angular;
        }
    }

    pub fn angular_factor(&self, theta: f64) -> f64 {
        0.5 + 0.5 * (theta - self.angle).cos()
    }

    pub fn active_color(&self, t: f64) -> Rgba {
        self.active_from.mix(&self.active_to, t)
    }

    pub fn inactive_color(&self, t: f64) -> Rgba {
        self.inactive_from.mix(&self.inactive_to, t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mix_endpoints() {
        let b = AnimatedGradientBorder::default();
        assert_eq!(b.active_color(0.0), b.active_from);
        assert_eq!(b.active_color(1.0), b.active_to);
    }

    #[test]
    fn loop_needs_explicit_repaint_opt_in() {
        let mut b = AnimatedGradientBorder::default();
        b.apply_angle_config(1.0, true, false);
        assert!(!b.needs_repaint());
        b.apply_angle_config(1.0, true, true);
        assert!(b.needs_repaint());
        b.style = BorderStyle::Solid;
        assert!(!b.needs_repaint());
    }
}
