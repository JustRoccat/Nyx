use std::fmt::Write as _;

use insta::assert_snapshot;
use nyx_ipc::SizeChange;
use wayland_client::protocol::wl_surface::WlSurface;

use super::client::ClientId;
use super::*;
use crate::layout::LayoutElement;
use crate::nyx::Niri;

fn format_window_sizes(niri: &Niri) -> String {
    let mut buf = String::new();
    for (_out, mapped) in niri.layout.windows() {
        let size = mapped.size();
        writeln!(&mut buf, "{} × {}", size.w, size.h).unwrap();
    }
    buf
}

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

#[test]
fn canvas_resize_applies_after_ack() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let id = f.add_client();

    let surface1 = create_window(&mut f, id, 100, 100);
    let surface2 = create_window(&mut f, id, 200, 200);
    f.double_roundtrip(id);

    let _ = f.client(id).window(&surface1).recent_configures();
    let _ = f.client(id).window(&surface2).recent_configures();

    // Issue a width resize on the active (second) canvas window.
    // It must not apply until the client acks, and the other window must be
    // left alone.
    f.niri()
        .layout
        .set_window_width(None, SizeChange::AdjustFixed(10));
    f.double_roundtrip(id);

    // The active (second) window got a configure with the adjusted tile
    // width (200 + 10); height is preserved.
    let window = f.client(id).window(&surface2);
    assert_snapshot!(
        window.format_recent_configures(),
        @"size: 210 × 200, bounds: 1920 × 1080, states: [Activated]"
    );
    let window = f.client(id).window(&surface1);
    assert_snapshot!(window.format_recent_configures(), @"");

    // Sizes stay put until the client acks (newest window first).
    assert_snapshot!(format_window_sizes(f.niri()), @r"
    200 × 200
    100 × 100
    ");

    // Ack the resize; it applies.
    let window = f.client(id).window(&surface2);
    window.set_size(210, 200);
    window.ack_last_and_commit();
    f.double_roundtrip(id);

    assert_snapshot!(format_window_sizes(f.niri()), @r"
    210 × 200
    100 × 100
    ");
}
