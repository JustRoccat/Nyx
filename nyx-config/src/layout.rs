use knuffel::errors::DecodeError;
use nyx_ipc::SizeChange;
use tracing::warn;

use crate::appearance::{Border, FocusRing, Shadow, DEFAULT_BACKGROUND_COLOR};
use crate::utils::{expect_only_children, Flag, MergeWith};
use crate::{BorderRule, Color, FloatOrInt, ShadowRule};

#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub focus_ring: FocusRing,
    pub border: Border,
    pub shadow: Shadow,
    pub preset_window_widths: Vec<PresetSize>,
    pub default_window_width: Option<PresetSize>,
    pub preset_window_heights: Vec<PresetSize>,
    pub empty_workspace_above_first: bool,
    pub gaps: f64,
    pub struts: Struts,
    pub background_color: Color,
    pub active_opacity: f32,
    pub inactive_opacity: f32,
    pub dim_inactive: bool,
    pub dim_strength: f32,
    pub rounding_power: f32,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            focus_ring: FocusRing::default(),
            border: Border::default(),
            shadow: Shadow::default(),
            preset_window_widths: vec![
                PresetSize::Proportion(1. / 3.),
                PresetSize::Proportion(0.5),
                PresetSize::Proportion(2. / 3.),
            ],
            default_window_width: None,
            empty_workspace_above_first: false,
            gaps: 16.,
            struts: Struts::default(),
            preset_window_heights: vec![
                PresetSize::Proportion(1. / 3.),
                PresetSize::Proportion(0.5),
                PresetSize::Proportion(2. / 3.),
            ],
            background_color: DEFAULT_BACKGROUND_COLOR,
            active_opacity: 1.0,
            inactive_opacity: 1.0,
            dim_inactive: false,
            dim_strength: 0.2,
            rounding_power: 2.0,
        }
    }
}

impl MergeWith<LayoutPart> for Layout {
    fn merge_with(&mut self, part: &LayoutPart) {
        merge!(
            (self, part),
            focus_ring,
            border,
            shadow,
            empty_workspace_above_first,
            gaps,
        );

        merge_clone!(
            (self, part),
            preset_window_widths,
            preset_window_heights,
            struts,
            background_color,
            dim_inactive,
        );

        if let Some(v) = part.active_opacity {
            self.active_opacity = v.0.clamp(0.0, 1.0) as f32;
        }
        if let Some(v) = part.inactive_opacity {
            self.inactive_opacity = v.0.clamp(0.0, 1.0) as f32;
        }
        if let Some(v) = part.dim_strength {
            self.dim_strength = v.0.clamp(0.0, 1.0) as f32;
        }
        if let Some(v) = part.rounding_power {
            self.rounding_power = v.0.clamp(1.0, 4.0) as f32;
        }

        if let Some(x) = part.default_window_width {
            self.default_window_width = x.0;
        }

        // Deprecated `preset-column-widths` / `default-column-width` aliases.
        // The new window-* names win when both are set.
        if part.preset_column_widths.is_some() {
            warn!("preset-column-widths is deprecated, rename it to preset-window-widths");
        }
        if part.default_column_width.is_some() {
            warn!("default-column-width is deprecated, rename it to default-window-width");
        }
        if part.preset_window_widths.is_none() {
            if let Some(widths) = part.preset_column_widths.clone() {
                self.preset_window_widths = widths;
            }
        }
        if part.default_window_width.is_none() {
            if let Some(x) = part.default_column_width {
                self.default_window_width = x.0;
            }
        }

        if self.preset_window_widths.is_empty() {
            self.preset_window_widths = Layout::default().preset_window_widths;
        }

        if self.preset_window_heights.is_empty() {
            self.preset_window_heights = Layout::default().preset_window_heights;
        }
    }
}

#[derive(knuffel::Decode, Debug, Default, Clone, PartialEq)]
pub struct LayoutPart {
    #[knuffel(child)]
    pub focus_ring: Option<BorderRule>,
    #[knuffel(child)]
    pub border: Option<BorderRule>,
    #[knuffel(child)]
    pub shadow: Option<ShadowRule>,
    #[knuffel(child, unwrap(children))]
    pub preset_window_widths: Option<Vec<PresetSize>>,
    #[knuffel(child)]
    pub default_window_width: Option<DefaultPresetSize>,
    /// Deprecated alias for `preset-window-widths`.
    #[knuffel(child, unwrap(children))]
    pub preset_column_widths: Option<Vec<PresetSize>>,
    /// Deprecated alias for `default-window-width`.
    #[knuffel(child)]
    pub default_column_width: Option<DefaultPresetSize>,
    #[knuffel(child, unwrap(children))]
    pub preset_window_heights: Option<Vec<PresetSize>>,
    #[knuffel(child)]
    pub empty_workspace_above_first: Option<Flag>,
    #[knuffel(child, unwrap(argument))]
    pub gaps: Option<FloatOrInt<0, 65535>>,
    #[knuffel(child)]
    pub struts: Option<Struts>,
    #[knuffel(child)]
    pub background_color: Option<Color>,
    #[knuffel(child, unwrap(argument))]
    pub active_opacity: Option<FloatOrInt<0, 1>>,
    #[knuffel(child, unwrap(argument))]
    pub inactive_opacity: Option<FloatOrInt<0, 1>>,
    #[knuffel(child, unwrap(argument))]
    pub dim_inactive: Option<bool>,
    #[knuffel(child, unwrap(argument))]
    pub dim_strength: Option<FloatOrInt<0, 1>>,
    #[knuffel(child, unwrap(argument))]
    pub rounding_power: Option<FloatOrInt<1, 4>>,
}

#[derive(knuffel::Decode, Debug, Clone, Copy, PartialEq)]
pub enum PresetSize {
    Proportion(#[knuffel(argument)] f64),
    Fixed(#[knuffel(argument)] i32),
}

impl From<PresetSize> for SizeChange {
    fn from(value: PresetSize) -> Self {
        match value {
            PresetSize::Proportion(prop) => SizeChange::SetProportion(prop * 100.),
            PresetSize::Fixed(fixed) => SizeChange::SetFixed(fixed),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DefaultPresetSize(pub Option<PresetSize>);

#[derive(knuffel::Decode, Debug, Default, Clone, Copy, PartialEq)]
pub struct Struts {
    #[knuffel(child, unwrap(argument), default)]
    pub left: FloatOrInt<-65535, 65535>,
    #[knuffel(child, unwrap(argument), default)]
    pub right: FloatOrInt<-65535, 65535>,
    #[knuffel(child, unwrap(argument), default)]
    pub top: FloatOrInt<-65535, 65535>,
    #[knuffel(child, unwrap(argument), default)]
    pub bottom: FloatOrInt<-65535, 65535>,
}

impl<S> knuffel::Decode<S> for DefaultPresetSize
where
    S: knuffel::traits::ErrorSpan,
{
    fn decode_node(
        node: &knuffel::ast::SpannedNode<S>,
        ctx: &mut knuffel::decode::Context<S>,
    ) -> Result<Self, DecodeError<S>> {
        expect_only_children(node, ctx);

        let mut children = node.children();

        if let Some(child) = children.next() {
            if let Some(unwanted_child) = children.next() {
                ctx.emit_error(DecodeError::unexpected(
                    unwanted_child,
                    "node",
                    "expected no more than one child",
                ));
            }
            PresetSize::decode_node(child, ctx).map(Some).map(Self)
        } else {
            Ok(Self(None))
        }
    }
}
