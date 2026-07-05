use motion::MotionEvent;

use crate::parser::{parse_line, Command, HomeAxes, MoveArgs};

/// Tunables for the naive feedrate-based timing model. None of this needs to
/// be physically precise: it only has to produce a plausible-looking,
/// monotonically increasing virtual clock for the renderer to animate along.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PrinterConfig {
    /// Used for G0/G1 moves that don't specify F yet (start of file).
    pub default_feedrate_mm_per_min: f32,
    /// Feedrate used for G28 homing moves.
    pub homing_feedrate_mm_per_min: f32,
    /// How long M109/M190 pause the timeline to simulate heating up.
    pub heatup_wait_s: f64,
}

impl Default for PrinterConfig {
    fn default() -> Self {
        Self {
            default_feedrate_mm_per_min: 1500.0,
            homing_feedrate_mm_per_min: 3000.0,
            heatup_wait_s: 15.0,
        }
    }
}

struct State {
    x: f32,
    y: f32,
    z: f32,
    e: f32,
    absolute_xyz: bool,
    absolute_e: bool,
    feedrate: f32,
    t: f64,
}

impl State {
    fn new(config: &PrinterConfig) -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            e: 0.0,
            absolute_xyz: true,
            absolute_e: true,
            feedrate: config.default_feedrate_mm_per_min,
            t: 0.0,
        }
    }

    fn resolve_target(&self, args: &MoveArgs) -> (f32, f32, f32, f32) {
        let resolve = |current: f32, delta: Option<f32>, absolute: bool| match delta {
            None => current,
            Some(v) if absolute => v,
            Some(v) => current + v,
        };
        (
            resolve(self.x, args.x, self.absolute_xyz),
            resolve(self.y, args.y, self.absolute_xyz),
            resolve(self.z, args.z, self.absolute_xyz),
            resolve(self.e, args.e, self.absolute_e),
        )
    }
}

/// Extrusion deltas below this are treated as "not depositing material" to
/// avoid flagging floating point noise as a print segment.
const EXTRUDE_EPSILON: f32 = 1e-4;

fn push_move(
    events: &mut Vec<MotionEvent>,
    state: &mut State,
    target: (f32, f32, f32, f32),
    feedrate_mm_per_min: f32,
    line: u32,
) {
    let (tx, ty, tz, te) = target;
    let dx = tx - state.x;
    let dy = ty - state.y;
    let dz = tz - state.z;
    let de = te - state.e;

    let cartesian_dist = (dx * dx + dy * dy + dz * dz).sqrt();
    let distance = if cartesian_dist > f32::EPSILON {
        cartesian_dist
    } else {
        de.abs()
    };

    let feedrate = if feedrate_mm_per_min > f32::EPSILON {
        feedrate_mm_per_min
    } else {
        60.0
    };
    let duration_s = (distance / feedrate * 60.0) as f64;

    state.x = tx;
    state.y = ty;
    state.z = tz;
    state.e = te;
    state.t += duration_s;

    events.push(MotionEvent {
        t: state.t,
        x: state.x,
        y: state.y,
        z: state.z,
        e: state.e,
        extruding: de > EXTRUDE_EPSILON,
        line,
    });
}

fn apply_home(
    events: &mut Vec<MotionEvent>,
    state: &mut State,
    axes: HomeAxes,
    config: &PrinterConfig,
    line: u32,
) {
    let target = (
        if axes.x { 0.0 } else { state.x },
        if axes.y { 0.0 } else { state.y },
        if axes.z { 0.0 } else { state.z },
        state.e,
    );
    push_move(events, state, target, config.homing_feedrate_mm_per_min, line);
}

/// Parses and time-steps a full gcode program into a `MotionEvent` stream.
///
/// The first event is always the printer's resting position at `t = 0`, so
/// consumers can always interpolate from a well-defined start.
pub fn simulate(gcode: &str, config: &PrinterConfig) -> Vec<MotionEvent> {
    let mut state = State::new(config);
    let mut events = vec![MotionEvent {
        t: 0.0,
        x: 0.0,
        y: 0.0,
        z: 0.0,
        e: 0.0,
        extruding: false,
        line: 0,
    }];

    for (line_index, line) in gcode.lines().enumerate() {
        let line_no = line_index as u32 + 1;
        let Some(cmd) = parse_line(line) else {
            continue;
        };

        match cmd {
            Command::RapidMove(args) | Command::LinearMove(args) => {
                if let Some(f) = args.f {
                    state.feedrate = f;
                }
                let target = state.resolve_target(&args);
                let feedrate = state.feedrate;
                push_move(&mut events, &mut state, target, feedrate, line_no);
            }
            Command::Home(axes) => apply_home(&mut events, &mut state, axes, config, line_no),
            Command::AbsolutePositioning => {
                state.absolute_xyz = true;
                state.absolute_e = true;
            }
            Command::RelativePositioning => {
                state.absolute_xyz = false;
                state.absolute_e = false;
            }
            Command::SetPosition(args) => {
                if let Some(x) = args.x {
                    state.x = x;
                }
                if let Some(y) = args.y {
                    state.y = y;
                }
                if let Some(z) = args.z {
                    state.z = z;
                }
                if let Some(e) = args.e {
                    state.e = e;
                }
            }
            Command::ExtruderAbsolute => state.absolute_e = true,
            Command::ExtruderRelative => state.absolute_e = false,
            Command::WaitHotendTemp { .. } | Command::WaitBedTemp { .. } => {
                // Emit a zero-motion event so playback interpolation holds
                // still during the wait (instead of creeping toward the next
                // move) and the gcode stream highlights the heating line.
                state.t += config.heatup_wait_s;
                events.push(MotionEvent {
                    t: state.t,
                    x: state.x,
                    y: state.y,
                    z: state.z,
                    e: state.e,
                    extruding: false,
                    line: line_no,
                });
            }
            Command::SetHotendTemp { .. } | Command::SetBedTemp { .. } => {}
        }
    }

    events
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_square_layer_produces_extruding_moves() {
        let gcode = "\
            G90\n\
            G1 X0 Y0 F3000\n\
            G1 X10 Y0 E1 F1200\n\
            G1 X10 Y10 E2\n\
            G1 X0 Y10 E3\n\
            G1 X0 Y0 E4\n\
        ";
        let events = simulate(gcode, &PrinterConfig::default());
        // initial rest event + 5 moves
        assert_eq!(events.len(), 6);
        assert!(events[2].extruding);
        assert!(events.last().unwrap().t > 0.0);
        // time never decreases
        for pair in events.windows(2) {
            assert!(pair[1].t >= pair[0].t);
        }
    }

    #[test]
    fn relative_extruder_mode_accumulates_from_current_e() {
        let gcode = "\
            G90\n\
            M83\n\
            G1 X10 E1 F600\n\
            G1 X20 E1\n\
        ";
        let events = simulate(gcode, &PrinterConfig::default());
        assert_eq!(events.last().unwrap().e, 2.0);
    }

    #[test]
    fn g92_resets_position_without_motion() {
        let gcode = "G92 E0\nG1 X5 E1 F600\n";
        let events = simulate(gcode, &PrinterConfig::default());
        assert_eq!(events.len(), 2);
        assert_eq!(events[1].e, 1.0);
    }

    #[test]
    fn homing_moves_to_origin() {
        let gcode = "G1 X50 Y50 F3000\nG28\n";
        let events = simulate(gcode, &PrinterConfig::default());
        let last = events.last().unwrap();
        assert_eq!((last.x, last.y, last.z), (0.0, 0.0, 0.0));
    }

    #[test]
    fn wait_temp_advances_virtual_clock() {
        let gcode = "M109 S200\n";
        let config = PrinterConfig {
            heatup_wait_s: 12.0,
            ..Default::default()
        };
        let events = simulate(gcode, &config);
        // The wait emits a zero-motion event so playback holds still on it.
        let last = events.last().unwrap();
        assert_eq!(last.t, 12.0);
        assert_eq!((last.x, last.y, last.z), (0.0, 0.0, 0.0));
        // and a following move starts after the wait
        let gcode2 = "M109 S200\nG1 X10 F600\n";
        let events2 = simulate(gcode2, &config);
        assert!(events2.last().unwrap().t >= 12.0);
    }
}
