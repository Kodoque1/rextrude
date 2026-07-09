//! Shared types produced by any printer backend (gcode simulator, firmware emulator)
//! and consumed by the rendering pipeline.

mod geometry;
mod layer;

pub use geometry::{build_ribbon_mesh, MeshData};
pub use layer::{extend_layers, split_into_layers, Layer};

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
    /// 1-based source line of the gcode command that produced this event;
    /// 0 when unknown (initial rest pose, firmware-emulator events).
    pub line: u32,
}

/// A full toolpath: every waypoint the head visits over the course of a print.
pub type Toolpath = Vec<MotionEvent>;

/// A point on the thermal timeline, timestamped in the same virtual seconds
/// as [`MotionEvent`]. Targets of 0 mean "off / unknown".
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThermalSample {
    pub t: f64,
    pub hotend_c: f32,
    pub hotend_target: f32,
    pub bed_c: f32,
    pub bed_target: f32,
}
