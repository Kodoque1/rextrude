//! Shared types produced by any printer backend (gcode simulator, firmware emulator)
//! and consumed by the rendering pipeline.

mod geometry;
mod layer;

pub use geometry::{build_ribbon_mesh, MeshData};
pub use layer::{split_into_layers, Layer};

/// A single waypoint of the print head, timestamped in virtual seconds.
///
/// Both backends (gcode planner and firmware emulator) emit the same
/// `MotionEvent` stream so the renderer never needs to know which one
/// produced it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionEvent {
    /// Virtual time in seconds since the print started.
    pub t: f64,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    /// Cumulative extruder position, in mm of filament.
    pub e: f32,
    /// Whether the segment leading up to this event deposited material.
    pub extruding: bool,
}

/// A full toolpath: every waypoint the head visits over the course of a print.
pub type Toolpath = Vec<MotionEvent>;
