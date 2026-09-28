pub mod spring;
pub mod tiling;
pub mod viewport;

pub use self::spring::{CameraSpring, SpringParams};
pub use self::tiling::{DynamicTiling, TilingKind, TilingRegion};
pub use self::viewport::{CanvasNode, CanvasRect, Viewport};

/// Unique id for a canvas node (maps 1:1 to a window id in the layout).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CanvasId(pub u64);

impl CanvasId {
    pub fn new(id: u64) -> Self {
        Self(id)
    }
}
