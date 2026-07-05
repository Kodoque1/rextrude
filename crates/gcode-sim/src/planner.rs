use motion::{MotionEvent, ThermalSample};

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
    /// First-order time constant of the hotend. Compressed far below a real
    /// printer's so heat-up doesn't dominate the demo timeline: M109 blocks
    /// until within [`TEMP_TOLERANCE_C`], i.e. roughly `tau * ln(dT/2)`.
    pub hotend_tau_s: f32,
    /// Same for the heated bed.
    pub bed_tau_s: f32,
    pub ambient_c: f32,
}

impl Default for PrinterConfig {
    fn default() -> Self {
        Self {
            default_feedrate_mm_per_min: 1500.0,
            homing_feedrate_mm_per_min: 3000.0,
            hotend_tau_s: 3.5,
            bed_tau_s: 5.0,
            ambient_c: 25.0,
        }
    }
}

/// M109/M190 unblock once the temperature is this close to target.
const TEMP_TOLERANCE_C: f32 = 2.0;
/// Thermal timeline sampling period (virtual seconds).
const SAMPLE_DT: f64 = 0.5;
/// Safety cap so a malformed target can never hang the wait loop.
const MAX_WAIT_S: f64 = 600.0;

/// A parsed + time-stepped gcode program: the motion stream plus the
/// simulated thermal timeline.
pub struct Simulation {
    pub toolpath: Vec<MotionEvent>,
    pub thermal: Vec<ThermalSample>,
}

/// Integrates first-order heating/cooling (`T' = (target - T) / tau`) along
/// the planner's virtual clock, emitting a sample every [`SAMPLE_DT`].
struct Thermal {
    hotend_c: f32,
    bed_c: f32,
    hotend_target: f32,
    bed_target: f32,
    clock: f64,
    samples: Vec<ThermalSample>,
}

impl Thermal {
    fn new(config: &PrinterConfig) -> Self {
        let mut thermal = Self {
            hotend_c: config.ambient_c,
            bed_c: config.ambient_c,
            hotend_target: 0.0,
            bed_target: 0.0,
            clock: 0.0,
            samples: Vec::new(),
        };
        thermal.push_sample();
        thermal
    }

    fn push_sample(&mut self) {
        // A little ripple once settled, so a "stable" trace still looks like
        // a live PID loop rather than a flat line.
        let ripple = |temp: f32, target: f32, phase: f32| {
            if target > 0.0 && (temp - target).abs() < 1.5 {
                temp + 0.4 * ((self.clock as f32) * phase).sin()
            } else {
                temp
            }
        };
        self.samples.push(ThermalSample {
            t: self.clock,
            hotend_c: ripple(self.hotend_c, self.hotend_target, 1.9),
            hotend_target: self.hotend_target,
            bed_c: ripple(self.bed_c, self.bed_target, 1.3),
            bed_target: self.bed_target,
        });
    }

    fn step(&mut self, dt: f64, config: &PrinterConfig) {
        let step_one = |temp: f32, target: f32, tau: f32| {
            let goal = if target > 0.0 { target } else { config.ambient_c };
            goal + (temp - goal) * (-dt as f32 / tau).exp()
        };
        self.hotend_c = step_one(self.hotend_c, self.hotend_target, config.hotend_tau_s);
        self.bed_c = step_one(self.bed_c, self.bed_target, config.bed_tau_s);
    }

    /// Catches the thermal timeline up with the motion clock.
    fn advance_to(&mut self, t: f64, config: &PrinterConfig) {
        while self.clock + SAMPLE_DT <= t {
            self.clock += SAMPLE_DT;
            self.step(SAMPLE_DT, config);
            self.push_sample();
        }
    }

    /// Blocks the virtual clock until the chosen heater is within tolerance
    /// of its target; returns the new clock time.
    fn wait_for(&mut self, hotend: bool, from_t: f64, config: &PrinterConfig) -> f64 {
        self.advance_to(from_t, config);
        let mut t = from_t.max(self.clock);
        let deadline = from_t + MAX_WAIT_S;
        loop {
            let (temp, target) = if hotend {
                (self.hotend_c, self.hotend_target)
            } else {
                (self.bed_c, self.bed_target)
            };
            if (temp - target).abs() <= TEMP_TOLERANCE_C || t >= deadline {
                return t;
            }
            t += SAMPLE_DT;
            self.clock = t;
            self.step(SAMPLE_DT, config);
            self.push_sample();
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

/// Convenience wrapper for callers that only need the motion stream.
pub fn simulate(gcode: &str, config: &PrinterConfig) -> Vec<MotionEvent> {
    simulate_full(gcode, config).toolpath
}

/// Parses and time-steps a full gcode program into a `MotionEvent` stream
/// plus the thermal timeline.
///
/// The first event is always the printer's resting position at `t = 0`, so
/// consumers can always interpolate from a well-defined start.
pub fn simulate_full(gcode: &str, config: &PrinterConfig) -> Simulation {
    let mut state = State::new(config);
    let mut thermal = Thermal::new(config);
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
                thermal.advance_to(state.t, config);
            }
            Command::Home(axes) => {
                apply_home(&mut events, &mut state, axes, config, line_no);
                thermal.advance_to(state.t, config);
            }
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
            Command::WaitHotendTemp { celsius } | Command::WaitBedTemp { celsius } => {
                let hotend = matches!(cmd, Command::WaitHotendTemp { .. });
                thermal.advance_to(state.t, config);
                if hotend {
                    thermal.hotend_target = celsius;
                } else {
                    thermal.bed_target = celsius;
                }
                state.t = thermal.wait_for(hotend, state.t, config);
                // Emit a zero-motion event so playback interpolation holds
                // still during the wait (instead of creeping toward the next
                // move) and the gcode stream highlights the heating line.
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
            Command::SetHotendTemp { celsius } => {
                thermal.advance_to(state.t, config);
                thermal.hotend_target = celsius;
            }
            Command::SetBedTemp { celsius } => {
                thermal.advance_to(state.t, config);
                thermal.bed_target = celsius;
            }
        }
    }

    // Let the trace settle a little past the last motion so the graph
    // doesn't cut off mid-slope.
    let end = state.t;
    thermal.advance_to(end, config);

    Simulation {
        toolpath: events,
        thermal: thermal.samples,
    }
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
        let config = PrinterConfig::default();
        let sim = simulate_full(gcode, &config);
        // The wait emits a zero-motion event so playback holds still on it,
        // once the hotend has reached the target.
        let last = *sim.toolpath.last().unwrap();
        assert!(last.t > 5.0 && last.t < 60.0, "wait was {}s", last.t);
        assert_eq!((last.x, last.y, last.z), (0.0, 0.0, 0.0));
        let final_temp = sim.thermal.last().unwrap().hotend_c;
        assert!((final_temp - 200.0).abs() <= 3.0, "hotend was {final_temp}");
        // and a following move starts after the wait
        let gcode2 = "M109 S200\nG1 X10 F600\n";
        let events2 = simulate(gcode2, &config);
        assert!(events2.last().unwrap().t >= last.t);
    }

    #[test]
    fn thermal_timeline_heats_and_holds() {
        let gcode = "M140 S60\nM190 S60\nG1 X10 F600\n";
        let sim = simulate_full(gcode, &PrinterConfig::default());
        let first = sim.thermal.first().unwrap();
        let last = sim.thermal.last().unwrap();
        assert_eq!(first.bed_c, PrinterConfig::default().ambient_c);
        assert!((last.bed_c - 60.0).abs() <= 3.0, "bed was {}", last.bed_c);
        // timeline is monotonic
        for pair in sim.thermal.windows(2) {
            assert!(pair[1].t >= pair[0].t);
        }
    }
}
