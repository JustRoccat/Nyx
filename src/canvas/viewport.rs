use std::time::Duration;

use super::spring::{CameraSpring, SpringParams};

#[derive(Debug, Clone)]
pub struct Viewport {
    pub cam_x: f64,
    pub cam_y: f64,
    pub zoom: f64,

    target_x: f64,
    target_y: f64,
    target_zoom: f64,

    spring_x: CameraSpring,
    spring_y: CameraSpring,
    spring_zoom: CameraSpring,

    pub min_scale: f64,
    pub max_scale: f64,

    pub zoom_speed: f64,

    pub keyboard_zoom_step: f64,
    pub zoom_to_center: bool,
    pub zoom_to_cursor: bool,
    pub smooth_camera: bool,
    pub overview_restore_zoom: bool,

    pub in_overview: bool,
    saved_zoom: f64,

    pub output_w: f64,
    pub output_h: f64,
}

impl Default for Viewport {
    fn default() -> Self {
        let params = SpringParams::default();
        Self {
            cam_x: 0.0,
            cam_y: 0.0,
            zoom: 1.0,
            target_x: 0.0,
            target_y: 0.0,
            target_zoom: 1.0,
            spring_x: CameraSpring::new(0.0, 0.0, params),
            spring_y: CameraSpring::new(0.0, 0.0, params),
            spring_zoom: CameraSpring::new(1.0, 1.0, params),
            min_scale: 0.1,
            max_scale: 3.0,
            zoom_speed: 0.12,
            keyboard_zoom_step: 1.0,
            zoom_to_center: true,
            zoom_to_cursor: true,
            smooth_camera: true,
            overview_restore_zoom: true,
            in_overview: false,
            saved_zoom: 1.0,
            output_w: 1920.0,
            output_h: 1080.0,
        }
    }
}

impl Viewport {
    pub fn new(output_w: f64, output_h: f64, params: SpringParams) -> Self {
        Self {
            output_w: output_w.max(1.0),
            output_h: output_h.max(1.0),
            spring_x: CameraSpring::new(0.0, 0.0, params),
            spring_y: CameraSpring::new(0.0, 0.0, params),
            spring_zoom: CameraSpring::new(1.0, 1.0, params),
            ..Self::default()
        }
    }

    pub fn configure(&mut self, min_scale: f64, max_scale: f64, zoom_speed: f64) {
        self.min_scale = min_scale.clamp(0.01, 10.0);
        self.max_scale = max_scale.clamp(self.min_scale, 10.0);
        self.zoom_speed = zoom_speed.clamp(0.01, 1.0);
        self.target_zoom = self.target_zoom.clamp(self.min_scale, self.max_scale);
    }

    pub fn configure_camera(&mut self, canvas: &nyx_config::Canvas) {
        self.configure(canvas.min_scale, canvas.max_scale, canvas.zoom_speed);
        self.keyboard_zoom_step = canvas.keyboard_zoom_step.clamp(0.1, 10.0);
        self.zoom_to_center = canvas.zoom_to_center;
        self.zoom_to_cursor = canvas.zoom_to_cursor;
        self.smooth_camera = canvas.smooth_camera;
        self.overview_restore_zoom = canvas.overview_restore_zoom;
    }

    pub fn set_output_size(&mut self, w: f64, h: f64) {
        self.output_w = w.max(1.0);
        self.output_h = h.max(1.0);
    }

    pub fn set_spring_params(&mut self, params: SpringParams) {
        self.spring_x.set_params(params);
        self.spring_y.set_params(params);
        self.spring_zoom.set_params(params);
    }

    pub fn canvas_to_screen(&self, wx: f64, wy: f64) -> (f64, f64) {
        let sx = (wx - self.cam_x) * self.zoom + self.output_w / 2.0;
        let sy = (wy - self.cam_y) * self.zoom + self.output_h / 2.0;
        (sx, sy)
    }

    pub fn screen_to_canvas(&self, sx: f64, sy: f64) -> (f64, f64) {
        let z = self.zoom.max(1e-6);
        let wx = (sx - self.output_w / 2.0) / z + self.cam_x;
        let wy = (sy - self.output_h / 2.0) / z + self.cam_y;
        (wx, wy)
    }

    pub fn rect_to_screen(&self, x: f64, y: f64, w: f64, h: f64) -> (f64, f64, f64, f64) {
        let (sx, sy) = self.canvas_to_screen(x, y);
        (sx, sy, w * self.zoom, h * self.zoom)
    }

    pub fn pan_by(&mut self, dx: f64, dy: f64) {
        self.target_x += dx;
        self.target_y += dy;
        self.spring_x.set_target(self.target_x);
        self.spring_y.set_target(self.target_y);
        self.snap_if_close();

        if self.in_overview {
            self.in_overview = false;
        }

        if !self.smooth_camera {
            self.snap_to_targets();
        }
    }

    pub fn drag_pan_by_screen(&mut self, dx_screen: f64, dy_screen: f64) {
        let z = self.zoom.max(1e-6);
        self.pan_by(-dx_screen / z, -dy_screen / z);
    }

    pub fn zoom_by_notches(&mut self, notches: f64, pivot_screen: (f64, f64)) {
        let old_zoom = self.target_zoom;
        let new_zoom = (old_zoom * (1.0 + self.zoom_speed).powf(notches))
            .clamp(self.min_scale, self.max_scale);
        let pivot = if self.zoom_to_cursor {
            pivot_screen
        } else {
            (self.output_w / 2.0, self.output_h / 2.0)
        };
        self.zoom_at_screen(new_zoom, pivot);
        if !self.smooth_camera {
            self.snap_to_targets();
        }
    }

    pub fn zoom_at_screen(&mut self, new_zoom: f64, pivot_screen: (f64, f64)) {
        let new_zoom = new_zoom.clamp(self.min_scale, self.max_scale);
        if (new_zoom - self.target_zoom).abs() < f64::EPSILON {
            return;
        }

        let old_z = self.zoom.max(1e-6);
        let (px, py) = pivot_screen;
        let world_x = (px - self.output_w / 2.0) / old_z + self.cam_x;
        let world_y = (py - self.output_h / 2.0) / old_z + self.cam_y;
        // New camera so that world point maps back to the same screen point.
        self.target_x = world_x - (px - self.output_w / 2.0) / new_zoom.max(1e-6);
        self.target_y = world_y - (py - self.output_h / 2.0) / new_zoom.max(1e-6);
        self.target_zoom = new_zoom;
        self.spring_x.set_target(self.target_x);
        self.spring_y.set_target(self.target_y);
        self.spring_zoom.set_target(self.target_zoom);
    }

    pub fn center_on(&mut self, wx: f64, wy: f64) {
        self.target_x = wx;
        self.target_y = wy;
        self.spring_x.set_target(wx);
        self.spring_y.set_target(wy);
    }

    pub fn target(&self) -> (f64, f64, f64) {
        (self.target_x, self.target_y, self.target_zoom)
    }

    pub fn restore_targets(&mut self, x: f64, y: f64, zoom: f64) {
        self.in_overview = false;
        self.target_x = x;
        self.target_y = y;
        self.target_zoom = zoom.clamp(self.min_scale, self.max_scale);
        self.spring_x.set_target(self.target_x);
        self.spring_y.set_target(self.target_y);
        self.spring_zoom.set_target(self.target_zoom);
    }

    pub fn center_on_rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.center_on(x + w / 2.0, y + h / 2.0);
    }

    pub fn toggle_overview(&mut self, overview_scale: f64) {
        if self.in_overview {
            self.in_overview = false;
            let restore = if self.overview_restore_zoom {
                self.saved_zoom.clamp(self.min_scale, self.max_scale)
            } else {
                1.0
            };
            self.zoom_at_center(restore);
        } else {
            self.in_overview = true;
            self.saved_zoom = self.target_zoom;
            self.zoom_at_center(overview_scale.clamp(self.min_scale, self.max_scale));
        }
        if !self.smooth_camera {
            self.snap_to_targets();
        }
    }

    /// Click in overview: center there and restore 1.0.
    pub fn overview_focus(&mut self, screen: (f64, f64)) {
        let (wx, wy) = self.screen_to_canvas(screen.0, screen.1);
        self.in_overview = false;
        self.center_on(wx, wy);
        self.zoom_at_center(1.0);
    }

    fn zoom_at_center(&mut self, new_zoom: f64) {
        let center = (self.output_w / 2.0, self.output_h / 2.0);
        self.zoom_at_screen(new_zoom, center);
    }

    pub fn reset_zoom(&mut self) {
        self.zoom_at_center(1.0);
    }

    pub fn zoom_to(&mut self, scale: f64) {
        self.zoom_at_center(scale.clamp(self.min_scale, self.max_scale));
        if !self.smooth_camera {
            self.snap_to_targets();
        }
    }

    pub fn home(&mut self) {
        self.in_overview = false;
        self.target_x = 0.0;
        self.target_y = 0.0;
        self.spring_x.set_target(0.0);
        self.spring_y.set_target(0.0);
        self.zoom_at_center(1.0);
    }

    pub fn advance(&mut self, dt: Duration) -> bool {
        if !self.smooth_camera {
            let moved = (self.cam_x - self.target_x).abs() > f64::EPSILON
                || (self.cam_y - self.target_y).abs() > f64::EPSILON
                || (self.zoom - self.target_zoom).abs() > f64::EPSILON;
            self.snap_to_targets();
            return moved;
        }
        let ax = self.spring_x.advance(dt);
        let ay = self.spring_y.advance(dt);
        let az = self.spring_zoom.advance(dt);
        self.cam_x = self.spring_x.value;
        self.cam_y = self.spring_y.value;
        self.zoom = self.spring_zoom.value;
        ax || ay || az
    }

    pub fn is_animating(&self) -> bool {
        self.spring_x.is_animating()
            || self.spring_y.is_animating()
            || self.spring_zoom.is_animating()
    }

    pub fn snap_to_targets(&mut self) {
        self.spring_x.snap_to_target();
        self.spring_y.snap_to_target();
        self.spring_zoom.snap_to_target();
        self.cam_x = self.spring_x.value;
        self.cam_y = self.spring_y.value;
        self.zoom = self.spring_zoom.value;
    }

    pub fn visible_origin(&self) -> (f64, f64) {
        self.screen_to_canvas(0.0, 0.0)
    }

    pub fn target_visible_rect(&self) -> (f64, f64, f64, f64) {
        let z = self.target_zoom.max(1e-6);
        let w = self.output_w / z;
        let h = self.output_h / z;
        (self.target_x - w / 2.0, self.target_y - h / 2.0, w, h)
    }

    pub fn visible_size(&self) -> (f64, f64) {
        let z = self.zoom.max(1e-6);
        (self.output_w / z, self.output_h / z)
    }

    pub fn spawn_center(&self, win_w: f64, win_h: f64) -> (f64, f64) {
        let (ox, oy) = self.visible_origin();
        let (vw, vh) = self.visible_size();
        (ox + (vw - win_w) / 2.0, oy + (vh - win_h) / 2.0)
    }

    fn snap_if_close(&mut self) {
        // Keep targets in sync for exact math; springs handle smoothing.
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CanvasRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl CanvasRect {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }

    pub fn center(&self) -> (f64, f64) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    pub fn contains(&self, px: f64, py: f64) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CanvasNode {
    pub rect: CanvasRect,
}

impl CanvasNode {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self {
            rect: CanvasRect::new(x, y, w, h),
        }
    }
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;

    #[test]
    fn screen_canvas_roundtrip() {
        let mut vp = Viewport::default();
        vp.cam_x = 100.0;
        vp.cam_y = -50.0;
        vp.zoom = 2.0;
        let (sx, sy) = vp.canvas_to_screen(100.0, -50.0);
        assert!((sx - 960.0).abs() < 1e-9);
        assert!((sy - 540.0).abs() < 1e-9);
        let (wx, wy) = vp.screen_to_canvas(sx, sy);
        assert!((wx - 100.0).abs() < 1e-9);
        assert!((wy + 50.0).abs() < 1e-9);
    }

    #[test]
    fn pivot_zoom_keeps_cursor_stable() {
        let mut vp = Viewport::default();
        vp.output_w = 1000.0;
        vp.output_h = 800.0;
        // Sync springs to targets for exact math.
        vp.spring_x.value = 0.0;
        vp.spring_y.value = 0.0;
        vp.spring_zoom.value = 1.0;
        let pivot = (700.0, 300.0);
        let (wx0, wy0) = vp.screen_to_canvas(pivot.0, pivot.1);
        vp.zoom_at_screen(2.0, pivot);
        // Simulate converged animation.
        vp.cam_x = vp.target_x;
        vp.cam_y = vp.target_y;
        vp.zoom = vp.target_zoom;
        let (wx1, wy1) = vp.screen_to_canvas(pivot.0, pivot.1);
        assert!((wx0 - wx1).abs() < 1e-9);
        assert!((wy0 - wy1).abs() < 1e-9);
    }

    #[test]
    fn spawn_is_centered() {
        let vp = Viewport::default();
        let (x, y) = vp.spawn_center(800.0, 600.0);

        assert!((x - (-960.0 + (1920.0 - 800.0) / 2.0)).abs() < 1e-9);
        assert!((y - (-540.0 + (1080.0 - 600.0) / 2.0)).abs() < 1e-9);
    }

    #[test]
    fn zoom_to_sets_target_and_clamps() {
        let mut vp = Viewport::default();
        vp.zoom_to(2.0);
        assert!((vp.target().2 - 2.0).abs() < 1e-9);
        vp.zoom_to(99.0);
        assert!((vp.target().2 - vp.max_scale).abs() < 1e-9);
        vp.zoom_to(0.0);
        assert!((vp.target().2 - vp.min_scale).abs() < 1e-9);
    }
}
