pub fn dim_factor(is_active: bool, dim_inactive: bool, strength: f32) -> f32 {
    if is_active || !dim_inactive {
        return 1.0;
    }
    (1.0 - strength.clamp(0.0, 1.0)).clamp(0.0, 1.0)
}

pub fn resolve_dim(
    is_active: bool,
    layout_dim_inactive: bool,
    layout_strength: f32,
    rule_dim_inactive: Option<bool>,
    rule_strength: Option<f32>,
) -> f32 {
    let dim = rule_dim_inactive.unwrap_or(layout_dim_inactive);
    let strength = rule_strength.unwrap_or(layout_strength);
    dim_factor(is_active, dim, strength)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_never_dimmed() {
        assert_eq!(dim_factor(true, true, 0.5), 1.0);
    }

    #[test]
    fn inactive_dim_applies_strength() {
        assert!((dim_factor(false, true, 0.2) - 0.8).abs() < 1e-6);
        assert_eq!(dim_factor(false, true, 1.0), 0.0);
    }

    #[test]
    fn disabled_dim_is_passthrough() {
        assert_eq!(dim_factor(false, false, 0.9), 1.0);
    }

    #[test]
    fn rule_overrides_layout() {
        assert_eq!(resolve_dim(false, true, 0.5, Some(false), None), 1.0);
        assert!((resolve_dim(false, false, 0.2, Some(true), Some(0.4)) - 0.6).abs() < 1e-6);
    }
}
