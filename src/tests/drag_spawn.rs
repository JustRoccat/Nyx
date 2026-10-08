//! Drag-to-spawn (skunkwm dragspawn port).
//!
//! Regression test: drawing a rectangle with `spawn-at-drag` must place the
//! next mapped window at the drawn canvas position with the drawn size,
//! without moving the camera.

use smithay::reexports::wayland_protocols_wlr::virtual_pointer::v1::client::zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1 as VirtualPointer;
use wayland_client::protocol::wl_pointer;

use super::client::ClientId;
use super::*;

const BTN_LEFT: u32 = 0x110;

fn fx(x: f64) -> u32 {
    (x * 256.) as u32
}

fn vpointer(f: &mut Fixture, id: ClientId, op: impl FnOnce(&VirtualPointer)) {
    let client = f.client(id);
    let manager = client.state.virtual_pointer_manager.as_ref().unwrap();
    let pointer = manager.create_virtual_pointer(None, &client.qh, ());
    op(&pointer);
}

#[test]
fn drag_spawn_places_window_at_drawn_rect() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let id = f.add_client();

    // Move the pointer where the drag will start.
    vpointer(&mut f, id, |pointer| {
        pointer.motion_absolute(1, fx(1400.), fx(100.), fx(1920.), fx(1080.));
        pointer.frame();
    });
    f.roundtrip(id);

    // Open drag-spawn like the spawn-at-drag action does.
    f.niri_state().open_drag_spawn(vec!["true".to_string()]);
    f.roundtrip(id);

    // Press, drag to (1700, 300), release: a 300x200 rect.
    vpointer(&mut f, id, |pointer| {
        pointer.button(2, BTN_LEFT, wl_pointer::ButtonState::Pressed);
        pointer.frame();
    });
    f.roundtrip(id);
    vpointer(&mut f, id, |pointer| {
        pointer.motion_absolute(3, fx(1700.), fx(300.), fx(1920.), fx(1080.));
        pointer.frame();
    });
    f.roundtrip(id);
    vpointer(&mut f, id, |pointer| {
        pointer.button(4, BTN_LEFT, wl_pointer::ButtonState::Released);
        pointer.frame();
    });
    f.roundtrip(id);

    // Camera is at the origin with zoom 1: canvas = screen - (960, 540).
    // The pending rect stays in screen coordinates; it is converted lazily
    // when the window maps.
    let pending = f
        .niri()
        .drag_spawn_pending
        .clone()
        .expect("drag must arm a pending placement");
    assert_eq!(pending.rect.loc, (1400., 100.).into());
    assert_eq!(pending.rect.size, (300., 200.).into());

    // A window mapping now must land exactly on the drawn rect.
    let window = f.client(id).create_window();
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(id);

    let window = f.client(id).window(&surface);
    window.attach_new_buffer();
    window.set_size(100, 100);
    window.ack_last_and_commit();
    f.double_roundtrip(id);

    // The window must have mapped by now.
    let mut count = 0;
    f.niri().layout.with_windows(|_, _, _, _| {
        count += 1;
    });
    assert_eq!(count, 1, "test window must have mapped");

    assert!(
        f.niri().drag_spawn_pending.is_none(),
        "pending placement must be consumed by the mapped window"
    );

    // Apply the requested drag size (its configure arrives after mapping).
    let window = f.client(id).window(&surface);
    let requested = window.format_recent_configures();
    assert!(
        requested.contains("size: 300 × 200"),
        "drag size must be requested from the client, got:\n{requested}"
    );
    let window = f.client(id).window(&surface);
    window.ack_last_and_commit();
    f.double_roundtrip(id);

    // The test client doesn't resize itself on configure like a real client
    // would; apply the requested size explicitly, then check the tile.
    let window = f.client(id).window(&surface);
    window.attach_new_buffer();
    window.set_size(300, 200);
    window.commit();
    f.double_roundtrip(id);

    let mut layouts = Vec::new();
    f.niri().layout.with_windows(|_, _, _, layout| {
        layouts.push((layout.pos_in_canvas_layout, layout.tile_size));
    });
    assert_eq!(layouts.len(), 1);
    assert_eq!(layouts[0].0, Some((440., -440.)));
    assert_eq!(layouts[0].1, (300., 200.));
}
