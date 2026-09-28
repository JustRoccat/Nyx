use crate::utils::{FloatOrInt, MergeWith};
use crate::FloatOrInt as F;

#[derive(Debug, Clone, PartialEq)]
pub struct Canvas {
    pub min_scale: f64,
    pub max_scale: f64,
    pub zoom_speed: f64,
    pub overview_scale: f64,
    pub pan_step: f64,
    pub spring: CanvasSpring,
    pub tiling: CanvasTiling,
    pub default_scale: f64,
    pub keyboard_zoom_step: f64,
    pub zoom_to_center: bool,
    pub zoom_to_cursor: bool,
    pub smooth_camera: bool,
    pub overview_restore_zoom: bool,
}

impl Default for Canvas {
    fn default() -> Self {
        Self {
            min_scale: 0.1,
            max_scale: 3.0,
            zoom_speed: 0.12,
            overview_scale: 0.4,
            pan_step: 200.0,
            spring: CanvasSpring::default(),
            tiling: CanvasTiling::default(),
            default_scale: 1.0,
            keyboard_zoom_step: 1.0,
            zoom_to_center: true,
            zoom_to_cursor: true,
            smooth_camera: true,
            overview_restore_zoom: true,
        }
    }
}

#[derive(knuffel::Decode, Debug, Default, Clone, PartialEq)]
pub struct CanvasPart {
    #[knuffel(child, unwrap(argument))]
    pub min_scale: Option<F<0, 10>>,
    #[knuffel(child, unwrap(argument))]
    pub max_scale: Option<F<0, 10>>,
    #[knuffel(child, unwrap(argument))]
    pub zoom_speed: Option<F<0, 10>>,
    #[knuffel(child, unwrap(argument))]
    pub overview_scale: Option<F<0, 10>>,
    #[knuffel(child, unwrap(argument))]
    pub pan_step: Option<F<0, 100000>>,
    #[knuffel(child)]
    pub spring: Option<CanvasSpringPart>,
    #[knuffel(child)]
    pub tiling: Option<CanvasTilingPart>,
    #[knuffel(child, unwrap(argument))]
    pub default_scale: Option<F<0, 10>>,
    #[knuffel(child, unwrap(argument))]
    pub keyboard_zoom_step: Option<F<0, 100>>,
    #[knuffel(child, unwrap(argument))]
    pub zoom_to_center: Option<bool>,
    #[knuffel(child, unwrap(argument))]
    pub zoom_to_cursor: Option<bool>,
    #[knuffel(child, unwrap(argument))]
    pub smooth_camera: Option<bool>,
    #[knuffel(child, unwrap(argument))]
    pub overview_restore_zoom: Option<bool>,
}

impl MergeWith<CanvasPart> for Canvas {
    fn merge_with(&mut self, part: &CanvasPart) {
        if let Some(v) = &part.min_scale {
            self.min_scale = v.0;
        }
        if let Some(v) = &part.max_scale {
            self.max_scale = v.0;
        }
        if let Some(v) = &part.zoom_speed {
            self.zoom_speed = v.0;
        }
        if let Some(v) = &part.overview_scale {
            self.overview_scale = v.0;
        }
        if let Some(v) = &part.pan_step {
            self.pan_step = v.0;
        }
        if let Some(p) = &part.spring {
            self.spring.merge_with(p);
        }
        if let Some(p) = &part.tiling {
            self.tiling.merge_with(p);
        }
        if let Some(v) = &part.default_scale {
            self.default_scale = v.0.clamp(0.01, 10.0);
        }
        if let Some(v) = &part.keyboard_zoom_step {
            self.keyboard_zoom_step = v.0.clamp(0.1, 10.0);
        }
        if let Some(v) = part.zoom_to_center {
            self.zoom_to_center = v;
        }
        if let Some(v) = part.zoom_to_cursor {
            self.zoom_to_cursor = v;
        }
        if let Some(v) = part.smooth_camera {
            self.smooth_camera = v;
        }
        if let Some(v) = part.overview_restore_zoom {
            self.overview_restore_zoom = v;
        }
        self.min_scale = self.min_scale.clamp(0.01, 10.0);
        self.max_scale = self.max_scale.clamp(self.min_scale, 10.0);
        self.overview_scale = self.overview_scale.clamp(self.min_scale, self.max_scale);
        self.zoom_speed = self.zoom_speed.clamp(0.01, 1.0);
        self.default_scale = self
            .default_scale
            .clamp(self.min_scale, self.max_scale);
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CanvasSpring {
    pub stiffness: f64,
    pub damping_ratio: f64,
    pub mass: f64,
}

impl Default for CanvasSpring {
    fn default() -> Self {
        Self {
            stiffness: 900.0,
            damping_ratio: 1.0,
            mass: 1.0,
        }
    }
}

#[derive(knuffel::Decode, Debug, Default, Clone, Copy, PartialEq)]
pub struct CanvasSpringPart {
    #[knuffel(child, unwrap(argument))]
    pub stiffness: Option<F<0, 100000>>,
    #[knuffel(child, unwrap(argument))]
    pub damping: Option<F<0, 100000>>,
    #[knuffel(child, unwrap(argument))]
    pub mass: Option<F<0, 100000>>,
}

impl MergeWith<CanvasSpringPart> for CanvasSpring {
    fn merge_with(&mut self, part: &CanvasSpringPart) {
        if let Some(v) = &part.stiffness {
            let f: f64 = v.0;
            self.stiffness = f.max(1.0);
        }
        if let Some(v) = &part.damping {
            let f: f64 = v.0;
            self.damping_ratio = f.max(0.01);
        }
        if let Some(v) = &part.mass {
            let f: f64 = v.0;
            self.mass = f.max(0.01);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CanvasTilingKind {
    #[default]
    Off,
    MasterStack,
    Grid,
    /// Hyprland-style dwindle (binary space partitioning).
    Dwindle,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CanvasTiling {
    pub enabled: bool,
    pub kind: CanvasTilingKind,
    pub gap: f64,
    pub master_ratio: f64,
}

impl Default for CanvasTiling {
    fn default() -> Self {
        Self {
            enabled: false,
            kind: CanvasTilingKind::Off,
            gap: 16.0,
            master_ratio: 0.6,
        }
    }
}

#[derive(knuffel::Decode, Debug, Default, Clone, PartialEq)]
pub struct CanvasTilingPart {
    #[knuffel(child)]
    pub off: bool,
    #[knuffel(child, unwrap(argument))]
    pub kind: Option<String>,
    #[knuffel(child, unwrap(argument))]
    pub gap: Option<FloatOrInt<0, 100000>>,
    #[knuffel(child, unwrap(argument))]
    pub master_ratio: Option<FloatOrInt<0, 100000>>,
}

impl MergeWith<CanvasTilingPart> for CanvasTiling {
    fn merge_with(&mut self, part: &CanvasTilingPart) {
        if part.off {
            self.enabled = false;
            self.kind = CanvasTilingKind::Off;
            return;
        }
        if let Some(kind) = &part.kind {
            match kind.as_str() {
                "master-stack" => {
                    self.kind = CanvasTilingKind::MasterStack;
                    self.enabled = true;
                }
                "grid" => {
                    self.kind = CanvasTilingKind::Grid;
                    self.enabled = true;
                }
                "dwindle" => {
                    self.kind = CanvasTilingKind::Dwindle;
                    self.enabled = true;
                }
                "off" => {
                    self.kind = CanvasTilingKind::Off;
                    self.enabled = false;
                }
                _ => {}
            }
        }
        if let Some(g) = &part.gap {
            self.gap = g.0.max(0.0);
        }
        if let Some(r) = &part.master_ratio {
            self.master_ratio = r.0.clamp(0.2, 0.8);
        }
    }
}

/// Eye-candy effects pipeline (rounded corners already in layout; blur in `blur`).
#[derive(Debug, Clone, PartialEq)]
pub struct Effects {
    pub corner_radius: f64,
    pub border_width: f64,
    pub border_active_gradient: Option<(String, String)>,
    pub shadow_radius: f64,
    pub shadow_opacity: f64,
    pub blur_passes: usize,
    pub blur_radius: f64,
    pub animations_enabled: bool,
}

impl Default for Effects {
    fn default() -> Self {
        Self {
            corner_radius: 12.0,
            border_width: 2.0,
            border_active_gradient: None,
            shadow_radius: 12.0,
            shadow_opacity: 0.5,
            blur_passes: 2,
            blur_radius: 8.0,
            animations_enabled: true,
        }
    }
}

#[derive(knuffel::Decode, Debug, Default, Clone, PartialEq)]
pub struct EffectsPart {
    #[knuffel(child, unwrap(argument))]
    pub corner_radius: Option<FloatOrInt<0, 100000>>,
    #[knuffel(child, unwrap(argument))]
    pub border_width: Option<FloatOrInt<0, 100000>>,
    #[knuffel(child, unwrap(argument))]
    pub shadow_radius: Option<FloatOrInt<0, 100000>>,
    #[knuffel(child, unwrap(argument))]
    pub shadow_opacity: Option<FloatOrInt<0, 100000>>,
    #[knuffel(child, unwrap(argument))]
    pub blur_passes: Option<u16>,
    #[knuffel(child, unwrap(argument))]
    pub blur_radius: Option<FloatOrInt<0, 100000>>,
}

impl MergeWith<EffectsPart> for Effects {
    fn merge_with(&mut self, part: &EffectsPart) {
        if let Some(v) = &part.corner_radius {
            self.corner_radius = v.0.max(0.0);
        }
        if let Some(v) = &part.border_width {
            self.border_width = v.0.max(0.0);
        }
        if let Some(v) = &part.shadow_radius {
            self.shadow_radius = v.0.max(0.0);
        }
        if let Some(v) = &part.shadow_opacity {
            self.shadow_opacity = v.0.clamp(0.0, 1.0);
        }
        if let Some(p) = part.blur_passes {
            self.blur_passes = (p as usize).clamp(1, 4);
        }
        if let Some(v) = &part.blur_radius {
            self.blur_radius = v.0.max(0.0);
        }
    }
}
