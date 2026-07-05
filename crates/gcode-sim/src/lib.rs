//! Parses gcode and time-steps it into a `motion::MotionEvent` stream using
//! a naive feedrate-based timing model. This is the "phase A" backend: no
//! firmware, no CPU emulation, just gcode in, animated toolpath out.

mod parser;
mod planner;

pub use parser::{parse_line, Command, HomeAxes, MoveArgs};
pub use planner::{simulate, PrinterConfig};
