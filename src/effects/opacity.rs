pub fn effective_alpha(
    rule_opacity: f32,
    layout_active: f32,
    layout_inactive: f32,
    rule_inactive: Option<f32>,
    is_active: bool,
    fullscreen_progress: f32,
) -> f32 {
    let base = rule_opacity.clamp(0.0, 1.0);
    let focus_opacity = if is_active {
        layout_active.clamp(0.0, 1.0)
    } else {
        rule_inactive.unwrap_or(layout_inactive).clamp(0.0, 1.0)
    };
    let alpha = base * focus_opacity;
    let p = fullscreen_progress.clamp(0.0, 1.0);
    alpha * (1.0 - p) + p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_uses_active_opacity() {
        assert!((effective_alpha(1.0, 1.0, 0.5, None, true, 0.0) - 1.0).abs() < 1e-6);
        assert!((effective_alpha(1.0, 1.0, 0.5, None, false, 0.0) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn rule_inactive_overrides_layout() {
        assert!((effective_alpha(1.0, 1.0, 0.5, Some(0.8), false, 0.0) - 0.8).abs() < 1e-6);
    }

    #[test]
    fn fullscreen_fades_to_opaque() {
        assert!((effective_alpha(0.5, 1.0, 0.5, None, false, 1.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn clamps() {
        assert_eq!(effective_alpha(5.0, 5.0, -1.0, None, true, 0.0), 1.0);
    }
}
