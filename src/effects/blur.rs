//! Dual-Kawase blur chain descriptor. GLES dispatch lives in `render_helpers`.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlurPass {
    pub scale: f64,
    pub downsample: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DualKawaseBlur {
    pub passes: usize,
    pub downsample_factor: f64,
    pub radius: f64,
    pub enabled: bool,
    pub vibrancy: f64,
    pub noise: f64,
    pub saturation: f64,
    pub contrast: f64,
    pub brightness: f64,
    pub new_optimizations: bool,
}

impl Default for DualKawaseBlur {
    fn default() -> Self {
        Self {
            passes: 2,
            downsample_factor: 2.0,
            radius: 8.0,
            enabled: false,
            vibrancy: 0.0,
            noise: 0.02,
            saturation: 1.5,
            contrast: 1.0,
            brightness: 0.0,
            new_optimizations: true,
        }
    }
}

impl DualKawaseBlur {
    pub fn new(passes: usize, radius: f64) -> Self {
        Self {
            passes: passes.clamp(1, 4),
            radius: radius.max(0.0),
            ..Self::default()
        }
    }

    pub fn from_config(blur: nyx_config::Blur) -> Self {
        Self {
            passes: (blur.passes as usize).clamp(1, 4),
            radius: blur.offset.max(0.0),
            enabled: !blur.off,
            vibrancy: blur.vibrancy.clamp(0.0, 1.0),
            noise: blur.noise.clamp(0.0, 1.0),
            saturation: blur.saturation.clamp(0.0, 4.0),
            contrast: blur.contrast.clamp(0.0, 4.0),
            brightness: blur.brightness.clamp(-1.0, 1.0),
            new_optimizations: blur.new_optimizations,
            ..Self::default()
        }
    }

    pub fn should_skip(&self, surface_opaque: bool, window_alpha: f32) -> bool {
        if !self.enabled {
            return true;
        }
        self.new_optimizations && surface_opaque && window_alpha >= 0.999
    }

    pub fn downsample_chain(&self) -> Vec<BlurPass> {
        (1..=self.passes)
            .map(|i| BlurPass {
                scale: 1.0 / self.downsample_factor.powi(i as i32),
                downsample: true,
            })
            .collect()
    }

    pub fn upsample_chain(&self) -> Vec<BlurPass> {
        (1..=self.passes)
            .rev()
            .map(|i| BlurPass {
                scale: 1.0 / self.downsample_factor.powi(i as i32),
                downsample: false,
            })
            .collect()
    }

    pub fn full_chain(&self) -> Vec<BlurPass> {
        let mut chain = self.downsample_chain();
        chain.extend(self.upsample_chain());
        chain
    }

    pub fn kawase_offset(&self, pass_scale: f64) -> (f64, f64) {
        let o = 0.5 + self.radius * 0.25 * pass_scale;
        (o, o)
    }
}

pub const KAWASE_GLSL: &str = r#"
vec4 kawaseBlur(sampler2D tex, vec2 uv, vec2 texel, float offset) {
    vec4 c = texture(tex, uv) * 0.36;
    c += texture(tex, uv + vec2(offset, offset) * texel) * 0.16;
    c += texture(tex, uv + vec2(-offset, offset) * texel) * 0.16;
    c += texture(tex, uv + vec2(offset, -offset) * texel) * 0.16;
    c += texture(tex, uv + vec2(-offset, -offset) * texel) * 0.16;
    return c;
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_lengths() {
        let b = DualKawaseBlur::new(2, 8.0);
        assert_eq!(b.downsample_chain().len(), 2);
        assert_eq!(b.full_chain().len(), 4);
    }

    #[test]
    fn skip_only_when_opaque_and_optimized() {
        let mut b = DualKawaseBlur::new(2, 8.0);
        b.enabled = true;
        assert!(b.should_skip(true, 1.0));
        assert!(!b.should_skip(false, 1.0));
        assert!(!b.should_skip(true, 0.5));
        b.new_optimizations = false;
        assert!(!b.should_skip(true, 1.0));
    }

    #[test]
    fn config_clamps() {
        let c = nyx_config::Blur {
            passes: 99,
            offset: 99.0,
            vibrancy: 5.0,
            contrast: 99.0,
            brightness: 5.0,
            ..Default::default()
        };
        let b = DualKawaseBlur::from_config(c);
        assert_eq!(b.passes, 4);
        assert!(b.vibrancy <= 1.0);
        assert!(b.contrast <= 4.0);
        assert!(b.brightness <= 1.0);
    }
}
