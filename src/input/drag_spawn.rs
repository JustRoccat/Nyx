use std::time::{Duration, Instant};

use smithay::backend::input::ButtonState;
use smithay::input::pointer::{
    AxisFrame, ButtonEvent, CursorImageStatus, GestureHoldBeginEvent, GestureHoldEndEvent,
    GesturePinchBeginEvent, GesturePinchEndEvent, GesturePinchUpdateEvent, GestureSwipeBeginEvent,
    GestureSwipeEndEvent, GestureSwipeUpdateEvent, GrabStartData as PointerGrabStartData,
    MotionEvent, PointerGrab, PointerInnerHandle, RelativeMotionEvent,
};
use smithay::input::SeatHandler;
use smithay::output::Output;
use smithay::utils::{Logical, Point, Rectangle};

use crate::nyx::State;
use crate::utils::spawning::spawn;

pub const DRAG_SPAWN_MIN: f64 = 20.;
pub const DRAG_SPAWN_TIMEOUT: Duration = Duration::from_secs(5);

const DRAG_BUTTON: u32 = 0x110;

#[derive(Debug, Clone)]
pub struct DragSpawnPending {
    pub output: Output,
    /// Drawn rectangle in output-local screen coordinates.
    ///
    /// Converted to canvas coordinates lazily when the window maps, so a
    /// camera flight between the drag and the mapping cannot shift it.
    pub rect: Rectangle<f64, Logical>,
    pub at: Instant,
}

impl DragSpawnPending {
    pub fn is_fresh(&self) -> bool {
        self.at.elapsed() < DRAG_SPAWN_TIMEOUT
    }
}

pub struct DragSpawnGrab {
    start_data: PointerGrabStartData<State>,
    command: Vec<String>,
    output: Output,
    anchor: Option<Point<f64, Logical>>,
    current: Point<f64, Logical>,
}

impl DragSpawnGrab {
    pub fn new(
        start_data: PointerGrabStartData<State>,
        command: Vec<String>,
        output: Output,
    ) -> Self {
        let current = start_data.location;
        Self {
            start_data,
            command,
            output,
            anchor: None,
            current,
        }
    }

    fn update_rect(&self, state: &mut State) {
        let Some(anchor) = self.anchor else {
            return;
        };
        let rect = normalized_rect(anchor, self.current);
        let geo = state.niri.global_space.output_geometry(&self.output);
        let Some(geo) = geo else {
            return;
        };
        let local = Rectangle::new(rect.loc - geo.loc.to_f64(), rect.size);
        state.niri.drag_spawn_rect = Some((self.output.clone(), local));
        state.niri.queue_redraw(&self.output);
    }

    fn finish(&mut self, data: &mut State) {
        let Some(anchor) = self.anchor else {
            return;
        };
        let rect = normalized_rect(anchor, self.current);
        if rect.size.w < DRAG_SPAWN_MIN || rect.size.h < DRAG_SPAWN_MIN {
            return;
        }

        if data.niri.layout.nyx_is_tiling() {
            return;
        }

        let geo = data.niri.global_space.output_geometry(&self.output);
        let Some(geo) = geo else {
            return;
        };
        let rect = Rectangle::new(
            rect.loc - geo.loc.to_f64(),
            rect.size,
        );

        data.niri.drag_spawn_pending = Some(DragSpawnPending {
            output: self.output.clone(),
            rect,
            at: Instant::now(),
        });

        // No activation token on purpose: an xdg-activate request from the new
        // window would focus-center the camera onto it, undoing the placement.
        spawn(self.command.clone(), None);
    }

    fn on_ungrab(&mut self, state: &mut State) {
        state.niri.drag_spawn_rect = None;
        state
            .niri
            .cursor_manager
            .set_cursor_image(CursorImageStatus::default_named());
        state.niri.queue_redraw_all();
    }
}

pub fn normalized_rect(a: Point<f64, Logical>, b: Point<f64, Logical>) -> Rectangle<f64, Logical> {
    Rectangle::from_extremities((a.x.min(b.x), a.y.min(b.y)), (a.x.max(b.x), a.y.max(b.y)))
}

impl PointerGrab<State> for DragSpawnGrab {
    fn motion(
        &mut self,
        data: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        _focus: Option<(<State as SeatHandler>::PointerFocus, Point<f64, Logical>)>,
        event: &MotionEvent,
    ) {
        handle.motion(data, None, event);
        self.current = event.location;
        self.update_rect(data);
    }

    fn relative_motion(
        &mut self,
        data: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        _focus: Option<(<State as SeatHandler>::PointerFocus, Point<f64, Logical>)>,
        event: &RelativeMotionEvent,
    ) {
        handle.relative_motion(data, None, event);
    }

    fn button(
        &mut self,
        data: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        event: &ButtonEvent,
    ) {
        if event.button == DRAG_BUTTON && event.state == ButtonState::Pressed {
            if self.anchor.is_none() {
                self.anchor = Some(handle.current_location());
                self.current = handle.current_location();
                self.update_rect(data);
            }
            return;
        }

        if event.button == DRAG_BUTTON
            && event.state == ButtonState::Released
            && self.anchor.is_some()
        {
            self.current = handle.current_location();
            self.finish(data);
            handle.unset_grab(self, data, event.serial, event.time, true);
        }
    }

    fn axis(
        &mut self,
        data: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        details: AxisFrame,
    ) {
        handle.axis(data, details);
    }

    fn frame(&mut self, data: &mut State, handle: &mut PointerInnerHandle<'_, State>) {
        handle.frame(data);
    }

    fn gesture_swipe_begin(
        &mut self,
        data: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        event: &GestureSwipeBeginEvent,
    ) {
        handle.gesture_swipe_begin(data, event);
    }

    fn gesture_swipe_update(
        &mut self,
        data: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        event: &GestureSwipeUpdateEvent,
    ) {
        handle.gesture_swipe_update(data, event);
    }

    fn gesture_swipe_end(
        &mut self,
        data: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        event: &GestureSwipeEndEvent,
    ) {
        handle.gesture_swipe_end(data, event);
    }

    fn gesture_pinch_begin(
        &mut self,
        data: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        event: &GesturePinchBeginEvent,
    ) {
        handle.gesture_pinch_begin(data, event);
    }

    fn gesture_pinch_update(
        &mut self,
        data: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        event: &GesturePinchUpdateEvent,
    ) {
        handle.gesture_pinch_update(data, event);
    }

    fn gesture_pinch_end(
        &mut self,
        data: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        event: &GesturePinchEndEvent,
    ) {
        handle.gesture_pinch_end(data, event);
    }

    fn gesture_hold_begin(
        &mut self,
        data: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        event: &GestureHoldBeginEvent,
    ) {
        handle.gesture_hold_begin(data, event);
    }

    fn gesture_hold_end(
        &mut self,
        data: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        event: &GestureHoldEndEvent,
    ) {
        handle.gesture_hold_end(data, event);
    }

    fn start_data(&self) -> &PointerGrabStartData<State> {
        &self.start_data
    }

    fn unset(&mut self, data: &mut State) {
        self.on_ungrab(data);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_normalizes_corners() {
        let rect = normalized_rect((10., 30.).into(), (4., 12.).into());
        assert_eq!(rect.loc, (4., 12.).into());
        assert_eq!(rect.size, (6., 18.).into());
    }

    #[test]
    fn tiny_drag_is_below_minimum() {
        let rect = normalized_rect((0., 0.).into(), (19., 100.).into());
        assert!(rect.size.w < DRAG_SPAWN_MIN);
    }
}
