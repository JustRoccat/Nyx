//! Pointer mapping through the canvas camera.
//!
//! Regression tests: at any camera zoom, the surface origin reported by
//! `contents_under()` must be stable (independent of where exactly inside the
//! window the pointer is) and equal to the rendered tile origin, so that
//! clicks land 1:1 on what is displayed.

use smithay::reexports::wayland_protocols_wlr::virtual_pointer::v1::client::zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1 as VirtualPointer;
use smithay::utils::{Logical, Point};
use wayland_client::protocol::{wl_pointer, wl_surface::WlSurface};

use super::client::ClientId;
use super::*;

fn create_window(f: &mut Fixture, id: ClientId, w: u16, h: u16) -> WlSurface {
    let window = f.client(id).create_window();
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(id);

    let window = f.client(id).window(&surface);
    window.attach_new_buffer();
    window.set_size(w, h);
    window.ack_last_and_commit();
    f.roundtrip(id);

    surface
}

fn surface_origin(f: &mut Fixture, pos: Point<f64, Logical>) -> Option<Point<f64, Logical>> {
    f.niri()
        .contents_under(pos)
        .surface
        .map(|(_, origin)| origin)
}

#[test]
fn contents_under_origin_is_stable_across_zoom() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let id = f.add_client();

    // 200x200 window at natural size, spawned at the view center: canvas
    // (-100, -100), camera at the origin.
    let _surface = create_window(&mut f, id, 200, 200);
    f.double_roundtrip(id);

    // Zoom out around the screen center and settle the camera springs.
    f.niri().layout.nyx_zoom_by_notches(-4., (960., 540.));
    f.niri_complete_animations();

    // Guard against a vacuous test: the zoom must actually differ from 1:1.
    let zoom = f
        .niri()
        .layout
        .workspaces()
        .next()
        .unwrap()
        .2
        .canvas_viewport()
        .zoom;
    assert!((zoom - 1.).abs() > 0.05, "zoom did not change: {zoom}");

    // Tile origin and size in view coordinates (= output coordinates, the
    // single output sits at global (0, 0)).
    let (tile_pos, tile_size) = {
        let niri = f.niri();
        let (_, _, ws) = niri.layout.workspaces().next().unwrap();
        let (tile, pos, _) = ws.tiles_with_render_positions().next().unwrap();
        (pos, tile.tile_size().to_f64())
    };

    // Two distinct interior points of the rendered window (fractions of the
    // scaled on-screen size).
    let view_size = tile_size.upscale(zoom);
    let p1 = tile_pos + Point::from((view_size.w * 0.25, view_size.h * 0.25));
    let p2 = tile_pos + Point::from((view_size.w * 0.75, view_size.h * 0.75));

    let o1 = surface_origin(&mut f, p1);
    let o2 = surface_origin(&mut f, p2);
    let o1 = o1.expect("pointer must hit the window");
    let o2 = o2.expect("pointer must hit the window");

    // The reported surface origin must not depend on the pointer position
    // within the window (the old math returned the pointer position itself).
    assert_eq!(o1, o2);
    // ...and must equal the rendered tile origin (borders off, buffer at 0).
    assert_eq!(o1, tile_pos);
}

#[test]
fn contents_under_origin_is_stable_on_second_output_across_zoom() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    f.add_output(2, (1920, 1080));
    // Second output sits at global x = 1920.
    let out2_x = 1920.;

    // Open the window on the second output.
    f.niri_focus_output(2);
    let id = f.add_client();
    let _surface = create_window(&mut f, id, 200, 200);
    f.double_roundtrip(id);

    // Zoom that output's workspace around its screen center and settle.
    f.niri().layout.nyx_zoom_by_notches(-4., (960., 540.));
    f.niri_complete_animations();

    let (zoom, tile_pos, tile_size) = {
        let output2 = f.niri_output(2);
        let niri = f.niri();
        let mon = niri.layout.monitor_for_output(&output2).unwrap();
        let ws = mon.active_workspace_ref();
        let (tile, pos, _) = ws.tiles_with_render_positions().next().unwrap();
        (ws.canvas_viewport().zoom, pos, tile.tile_size().to_f64())
    };
    assert!((zoom - 1.).abs() > 0.05, "zoom did not change: {zoom}");

    // Interior points in output-2-local coordinates, mapped to global.
    let view_size = tile_size.upscale(zoom);
    let lp1 = tile_pos + Point::from((view_size.w * 0.25, view_size.h * 0.25));
    let lp2 = tile_pos + Point::from((view_size.w * 0.75, view_size.h * 0.75));
    let p1 = lp1 + Point::from((out2_x, 0.));
    let p2 = lp2 + Point::from((out2_x, 0.));

    let o1 = surface_origin(&mut f, p1).expect("pointer must hit the window");
    let o2 = surface_origin(&mut f, p2).expect("pointer must hit the window");

    assert_eq!(o1, o2);
    assert_eq!(o1, tile_pos + Point::from((out2_x, 0.)));
}

/// Press/release coherence through a virtual pointer.
///
/// Clicking must not move the camera while buttons are held: press, motion
/// and release all have to map through one frozen transform, otherwise the
/// press and the release land on different surface spots (and the window
/// jumps under the cursor on grab). The deferred focus flight runs once all
/// buttons are up.
#[test]
fn click_freezes_camera_until_release() {
    const BTN_LEFT: u32 = 0x110;

    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let id = f.add_client();

    // 200x200 window spawned at the view center; camera on its center.
    let _surface = create_window(&mut f, id, 200, 200);
    f.double_roundtrip(id);

    // Pan away (window stays visible, off-center).
    f.niri().layout.nyx_pan_by(300., 0.);
    f.niri_complete_animations();
    let target_before = layout_target(&mut f);
    assert_eq!(target_before, (300., 0., 1.));

    // Move the pointer over the window middle: view (560..760, 440..640).
    vpointer(&mut f, id, |pointer| {
        pointer.motion_absolute(1, fx(660.), fx(540.), fx(1920.), fx(1080.));
        pointer.frame();
    });
    f.roundtrip(id);

    // Press: focus engages, camera stays frozen, deferral arms.
    vpointer(&mut f, id, |pointer| {
        pointer.button(2, BTN_LEFT, wl_pointer::ButtonState::Pressed);
        pointer.frame();
    });
    f.roundtrip(id);

    assert!(f.niri().layout.camera_center_deferred());
    assert_eq!(layout_target(&mut f), target_before);
    assert!(f.niri().pointer_contents.surface.is_some());

    // Release: deferred flight runs, camera centers on the window middle.
    vpointer(&mut f, id, |pointer| {
        pointer.button(3, BTN_LEFT, wl_pointer::ButtonState::Released);
        pointer.frame();
    });
    f.roundtrip(id);

    assert!(!f.niri().layout.camera_center_deferred());
    assert_eq!(layout_target(&mut f), (0., 0., 1.));
}

fn layout_target(f: &mut Fixture) -> (f64, f64, f64) {
    f.niri()
        .layout
        .workspaces()
        .next()
        .unwrap()
        .2
        .canvas_viewport()
        .target()
}

/// Wayland fixed-point encoding for virtual pointer coordinates.
fn fx(x: f64) -> u32 {
    (x * 256.) as u32
}

fn vpointer(f: &mut Fixture, id: ClientId, op: impl FnOnce(&VirtualPointer)) {
    let client = f.client(id);
    let manager = client.state.virtual_pointer_manager.as_ref().unwrap();
    let pointer = manager.create_virtual_pointer(None, &client.qh, ());
    op(&pointer);
}

/// End-to-end: the surface-local coordinates the client receives must equal
/// the exact mapping at any camera zoom (compositor computation + Smithay
/// delivery together).
#[test]
fn client_receives_exact_surface_coords_across_zoom() {
    use super::client::PointerEv;

    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let id = f.add_client();
    assert!(
        f.client(id).state.pointer.is_some(),
        "test server must advertise wl_seat"
    );

    // 200x200 window spawned at the view center; camera on its center.
    let _surface = create_window(&mut f, id, 200, 200);
    f.double_roundtrip(id);

    // Zoom out and settle.
    f.niri().layout.nyx_zoom_by_notches(-4., (960., 540.));
    f.niri_complete_animations();

    // Ground truth from layout state.
    let (zoom, tile_pos, tile_size, buf) = {
        let niri = f.niri();
        let (_, _, ws) = niri.layout.workspaces().next().unwrap();
        let zoom = ws.canvas_viewport().zoom;
        let (tile, pos, _) = ws.tiles_with_render_positions().next().unwrap();
        (zoom, pos, tile.tile_size().to_f64(), tile.buf_loc())
    };
    assert!((zoom - 1.).abs() > 0.05, "zoom did not change: {zoom}");

    // Interior point (25%) in view coordinates.
    let view_size = tile_size.upscale(zoom);
    let p = tile_pos + Point::from((view_size.w * 0.25, view_size.h * 0.25));
    // Expected surface-local coordinates (independent computation).
    let expected = ((p - tile_pos).downscale(zoom) - buf).to_f64();

    // Move there and drain all events.
    vpointer(&mut f, id, |pointer| {
        pointer.motion_absolute(1, fx(p.x), fx(p.y), fx(1920.), fx(1080.));
        pointer.frame();
    });
    f.double_roundtrip(id);

    // The client must have received (near-)exact coordinates.
    let log = f.client(id).state.pointer_log.clone();
    let logged = log.borrow();
    let last = logged.iter().rev().find_map(|ev| match ev {
        PointerEv::Motion { x, y } | PointerEv::Enter { x, y } => Some((*x, *y)),
        PointerEv::Leave => None,
    });
    let (rx, ry) = last.expect("client must receive pointer coordinates");
    assert!(
        (rx - expected.x).abs() < 1.0 && (ry - expected.y).abs() < 1.0,
        "client got ({rx},{ry}), expected ({},{})",
        expected.x,
        expected.y
    );
}

/// Hover focus must not move the camera.
///
/// With focus-follows-mouse, moving onto another window focuses it but the
/// view has to stay put; otherwise windows chase the cursor and pointer
/// mapping shifts under a static pointer (worse the further zoomed out).
#[test]
fn hover_focus_does_not_move_camera() {
    use nyx_config::input::FocusFollowsMouse;
    use nyx_config::Config;
    use smithay::desktop::Window;

    let mut config = Config::default();
    config.input.focus_follows_mouse = Some(FocusFollowsMouse {
        max_scroll_amount: None,
    });
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1920, 1080));
    let id = f.add_client();

    // Window 1 at the view center; camera on its center.
    let _s1 = create_window(&mut f, id, 200, 200);
    f.double_roundtrip(id);

    // Pan away and open window 2 at the new view center (separated tiles).
    f.niri().layout.nyx_pan_by(1000., 0.);
    f.niri_complete_animations();
    let _s2 = create_window(&mut f, id, 200, 200);
    f.double_roundtrip(id);

    // Zoom out around the screen center so both windows stay visible.
    f.niri().layout.nyx_zoom_by_notches(-6., (960., 540.));
    f.niri_complete_animations();

    // Focus window 1 (the older one); camera flies onto it.
    let win1: Window = {
        let niri = f.niri();
        let active = niri.layout.focus().unwrap().window.clone();
        niri.layout
            .windows()
            .find(|(_, m)| m.window != active)
            .map(|(_, m)| m.window.clone())
            .unwrap()
    };
    f.niri().layout.activate_window(&win1);
    f.niri_complete_animations();
    let cam_before = layout_target(&mut f);

    // Window 2 middle in view coordinates (the tile that is not window 1).
    let (w2view, w2size) = {
        let niri = f.niri();
        let (_, _, ws) = niri.layout.workspaces().next().unwrap();
        let zoom = ws.canvas_viewport().zoom;
        let (tile, pos, _) = ws
            .tiles_with_render_positions()
            .find(|(tile, _, _)| tile.window().window != win1)
            .expect("window 2 must be laid out");
        (pos, tile.tile_size().to_f64().upscale(zoom))
    };
    let hover = w2view + Point::from((w2size.w / 2., w2size.h / 2.));

    // Hover window 2: focus must engage with the camera frozen.
    vpointer(&mut f, id, |pointer| {
        pointer.motion_absolute(1, fx(hover.x), fx(hover.y), fx(1920.), fx(1080.));
        pointer.frame();
    });
    f.roundtrip(id);

    assert!(f.niri().pointer_contents.surface.is_some());
    assert_eq!(layout_target(&mut f), cam_before);
}
