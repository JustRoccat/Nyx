use std::time::Duration;

use crate::animation::{Spring, SpringParams as BaseParams};

/// Spring parameters (user-facing, KDL-configurable).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpringParams {
    pub stiffness: f64,
    pub damping_ratio: f64,
    pub mass: f64,
    pub epsilon: f64,
}

impl Default for SpringParams {
    fn default() -> Self {
        Self {
            stiffness: 900.0,
            damping_ratio: 1.0,
            mass: 1.0,
            epsilon: 0.001,
        }
    }
}

impl SpringParams {
    pub fn new(stiffness: f64, damping_ratio: f64, mass: f64) -> Self {
        Self {
            stiffness: stiffness.max(1.0),
            damping_ratio: damping_ratio.max(0.01),
            mass: mass.max(0.01),
            epsilon: 0.001,
        }
    }

    fn to_base(self) -> BaseParams {
        let scaled_stiffness = self.stiffness / self.mass;
        BaseParams::new(self.damping_ratio, scaled_stiffness, self.epsilon)
    }
}

#[derive(Debug, Clone)]
pub struct CameraSpring {
    pub value: f64,
    target: f64,
    velocity: f64,
    params: SpringParams,

    elapsed: Duration,
    start_value: f64,
    start_velocity: f64,
}

impl CameraSpring {
    pub fn new(value: f64, target: f64, params: SpringParams) -> Self {
        Self {
            value,
            target,
            velocity: 0.0,
            params,
            elapsed: Duration::ZERO,
            start_value: value,
            start_velocity: 0.0,
        }
    }

    pub fn set_target(&mut self, target: f64) {
        if (target - self.target).abs() < f64::EPSILON {
            return;
        }

        let v = self.instant_velocity();
        self.start_value = self.value;
        self.start_velocity = v;
        self.target = target;
        self.elapsed = Duration::ZERO;
    }

    pub fn set_params(&mut self, params: SpringParams) {
        self.params = params;
        self.start_value = self.value;
        self.start_velocity = self.instant_velocity();
        self.elapsed = Duration::ZERO;
    }

    pub fn advance(&mut self, dt: Duration) -> bool {
        self.elapsed += dt;
        let spring = Spring {
            from: self.start_value,
            to: self.target,
            initial_velocity: self.start_velocity,
            params: self.params.to_base(),
        };
        let new_value = spring.value_at(self.elapsed);
        // Numerical velocity estimate.
        let dt_s = dt.as_secs_f64().max(1e-4);
        self.velocity = (new_value - self.value) / dt_s;
        self.value = new_value;
        if (self.target - self.value).abs() <= self.params.epsilon && self.velocity.abs() <= 0.5 {
            self.value = self.target;
            self.velocity = 0.0;
            return false;
        }
        // Safety: clamp runaway.
        if !self.value.is_finite() {
            self.value = self.target;
            self.velocity = 0.0;
            return false;
        }
        true
    }

    pub fn is_animating(&self) -> bool {
        (self.target - self.value).abs() > self.params.epsilon
    }

    pub fn snap_to_target(&mut self) {
        self.value = self.target;
        self.velocity = 0.0;
        self.start_value = self.target;
        self.start_velocity = 0.0;
        self.elapsed = Duration::ZERO;
    }

    fn instant_velocity(&self) -> f64 {
        self.velocity
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spring_converges() {
        let mut s = CameraSpring::new(0.0, 100.0, SpringParams::default());
        s.set_target(100.0);
        s = CameraSpring::new(0.0, 0.0, SpringParams::default());
        s.set_target(100.0);
        for _ in 0..600 {
            s.advance(Duration::from_millis(16));
        }
        assert!((s.value - 100.0).abs() < 1.0, "value={}", s.value);
    }
}
