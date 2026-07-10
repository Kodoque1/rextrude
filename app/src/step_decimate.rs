//! Collapses the firmware backend's raw per-microstep step-event stream
//! (`RampsBoard.recordStep` in `emu/src/board.ts` emits one event per
//! microstep on any axis -- e.g. 80 steps/mm on X/Y) into render-scale
//! `MotionEvent`s, the same order of magnitude of density the gcode-sim
//! backend produces (one event per G-move). Without this, `PrintState`'s
//! `toolpath` grows unbounded at tens of thousands of events per simulated
//! second, and every downstream consumer -- especially the per-frame layer
//! mesh rebuild in `layers.rs` -- ends up doing far more work than the
//! visible geometry needs.
//!
//! This also fixes an attribution defect in the raw stream: naively
//! computing `extruding` per raw event (as the old `drive_firmware` did)
//! reads `false` on every XY/Z-only microstep and `true` on every E-only
//! microstep -- but an E-only microstep has zero XY displacement, so
//! `build_ribbon_mesh` (which requires both `extruding` and nonzero XY
//! length) skips essentially every segment. Computing `extruding` against
//! the last *kept* event, as this module does, fixes that.
//!
//! `firmware.rs` (the only production caller) is `#![cfg(target_arch =
//! "wasm32")]`, but this module deliberately isn't, so its unit tests run on
//! a native `cargo test`. That means a native, non-test build sees this
//! module as genuinely unused -- allow dead_code there; wasm32 keeps the
//! lint live, since that's where the real caller is.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

/// Min XY travel between kept firmware step events, in mm. ~1/9 of
/// `FILAMENT_WIDTH` (0.45mm in layers.rs): an X/Y microstep is 0.0125mm at
/// 80 steps/mm, so this collapses the raw stream by roughly 4-6x while
/// staying visually smooth at ribbon width, and drops pure-E microstep
/// events (zero XY travel) entirely.
pub const DECIMATE_XY_EPSILON: f32 = 0.05;

/// Extrusion deltas below this are noise, not real deposition -- mirrors
/// gcode-sim's own epsilon so both backends agree on what "extruding" means.
pub const EXTRUDE_EPSILON: f32 = 1e-4;

/// Feed raw per-microstep firmware events through `accept` in order; it
/// returns `Some(extruding)` for events that should become a `MotionEvent`
/// in the toolpath, `None` for events it collapsed away. `z` isn't a
/// parameter here at all -- it never factors into the keep/drop decision
/// (see `accept`'s doc), and the caller already has it on hand for
/// constructing the `MotionEvent` from a kept result.
///
/// Kept events are a subsequence of the raw stream, so if the raw stream's
/// `t` is nondecreasing (it is: `cycle / cycles_per_second` in
/// `drive_firmware`), so is the kept stream's -- `PrintState::current_index`
/// binary-searches on `t` and stays valid.
#[derive(Default)]
pub struct StepDecimator {
    last_x: f32,
    last_y: f32,
    last_e: f32,
    primed: bool,
}

impl StepDecimator {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// `frame_final` marks the last raw event drained in this call to
    /// `drive_firmware` -- used to flush the head's current X/Y/E once per
    /// frame even during a long run of sub-epsilon travel, so the on-screen
    /// head doesn't visibly freeze between real segments.
    ///
    /// Z is deliberately never part of the decision (and isn't a parameter):
    /// a frame-final flush mid-z-hop would otherwise commit a one-or-two-
    /// event Z jump that `split_into_layers`' z-threshold would read as a
    /// spurious extra layer. A z-hop with no XY/E change alongside it simply
    /// produces no kept event until the next real move, which then carries
    /// the hop's already-current z (read directly from the caller's own
    /// state, not from this decimator) -- one clean jump instead of a
    /// stutter. The on-screen head visually lags the real z by at most one
    /// frame's worth of hop time (well under 100ms).
    pub fn accept(&mut self, x: f32, y: f32, e: f32, frame_final: bool) -> Option<bool> {
        if !self.primed {
            self.primed = true;
            self.last_x = x;
            self.last_y = y;
            self.last_e = e;
            return Some(false);
        }

        let dx = x - self.last_x;
        let dy = y - self.last_y;
        let moved_enough = dx * dx + dy * dy >= DECIMATE_XY_EPSILON * DECIMATE_XY_EPSILON;
        let flush = frame_final && (x != self.last_x || y != self.last_y || e != self.last_e);
        if !moved_enough && !flush {
            return None;
        }

        // Computed against the last *kept* event (see module doc): a kept
        // segment spanning both XY and E microsteps correctly reads as
        // extruding, unlike attributing it per raw microstep.
        let extruding = e - self.last_e > EXTRUDE_EPSILON;
        self.last_x = x;
        self.last_y = y;
        self.last_e = e;
        Some(extruding)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use motion::{split_into_layers, MotionEvent};

    /// Drives a decimator over raw (x, y, e, frame_final) tuples, returning
    /// the kept (x, y, e, extruding) events.
    fn run(raw: &[(f32, f32, f32, bool)]) -> Vec<(f32, f32, f32, bool)> {
        let mut dec = StepDecimator::default();
        raw.iter()
            .filter_map(|&(x, y, e, frame_final)| {
                dec.accept(x, y, e, frame_final)
                    .map(|extruding| (x, y, e, extruding))
            })
            .collect()
    }

    #[test]
    fn straight_move_decimates_to_epsilon_grid() {
        // 1mm X move as 80 microsteps of 0.0125mm (80 steps/mm), with
        // proportional E so every microstep extrudes measurably.
        let raw: Vec<(f32, f32, f32, bool)> = (0..=80)
            .map(|i| (i as f32 * 0.0125, 0.0, i as f32 * 0.0004, false))
            .collect();
        let kept = run(&raw);

        // Prime + roughly one kept event per 0.05mm (4 microsteps) + a
        // possible tail remainder.
        assert!(
            kept.len() <= 1 + 20 + 1,
            "expected roughly 21 kept events, got {}",
            kept.len()
        );
        assert!(
            kept.len() > 1,
            "the move should decimate to more than just the prime event"
        );

        for pair in kept.windows(2) {
            let (prev, cur) = (pair[0], pair[1]);
            let dx = cur.0 - prev.0;
            assert!(
                dx >= DECIMATE_XY_EPSILON - f32::EPSILON,
                "kept events should be at least epsilon apart in X, got dx={dx}"
            );
            assert!(
                cur.3,
                "every kept event after the prime should read as extruding"
            );
        }
    }

    #[test]
    fn pure_e_microsteps_produce_no_events_until_flush() {
        let mut dec = StepDecimator::default();
        assert_eq!(dec.accept(0.0, 0.0, 0.0, false), Some(false)); // prime

        for i in 1..=50 {
            let kept = dec.accept(0.0, 0.0, i as f32 * 0.001, false);
            assert_eq!(
                kept, None,
                "a pure-E microstep with no frame-final flag must not be kept"
            );
        }

        let flushed = dec.accept(0.0, 0.0, 51.0 * 0.001, true);
        assert_eq!(
            flushed,
            Some(true),
            "a frame-final flush with an E delta must be kept and read as extruding"
        );
    }

    #[test]
    fn retract_then_travel_reads_not_extruding() {
        let mut dec = StepDecimator::default();
        dec.accept(0.0, 0.0, 0.0, false); // prime

        // Extrude a real move.
        let extrude = dec
            .accept(0.1, 0.0, 0.02, false)
            .expect("a 0.1mm move exceeds the XY epsilon");
        assert!(extrude);

        // Retract: E-only, no XY movement, not frame-final -- collapsed.
        assert_eq!(dec.accept(0.1, 0.0, -0.02, false), None);

        // Travel far enough to be kept; the retract's E delta must still be
        // reflected against the last kept event, reading not-extruding.
        let travel = dec
            .accept(0.3, 0.0, -0.02, false)
            .expect("a 0.2mm travel exceeds the XY epsilon");
        assert!(
            !travel,
            "a travel move after a retract must not read as extruding"
        );
    }

    #[test]
    fn z_hop_is_a_single_clean_jump() {
        let mut dec = StepDecimator::default();
        dec.accept(0.0, 0.0, 0.0, false); // prime
        let first = dec
            .accept(0.1, 0.0, 0.02, false)
            .expect("XY move exceeds epsilon");
        assert!(first);

        // Pure-z microsteps (z isn't passed to accept at all -- it's not
        // part of the decision), including one frame-final mid-hop: no XY/E
        // change alongside them, so none should be kept.
        for i in 1..=80 {
            let frame_final = i == 40; // a frame boundary lands mid-hop
            let kept = dec.accept(0.1, 0.0, 0.02, frame_final);
            assert_eq!(
                kept, None,
                "a pure-z microstep must never be kept, even frame-final"
            );
        }

        // First XY move after the hop is kept normally; the caller attaches
        // whatever z it currently has (already at the hopped height).
        let after_hop = dec
            .accept(0.3, 0.0, 0.06, false)
            .expect("XY move after the hop exceeds epsilon");
        assert!(after_hop);

        // Build the equivalent MotionEvent stream a caller would produce
        // (attaching its own current z to each kept result) and confirm the
        // hop reads as exactly one clean layer boundary, not a stutter.
        let events: Vec<MotionEvent> = [
            (0.0, 0.0, 0.2, 0.0),
            (0.1, 0.0, 0.2, 0.02),
            (0.3, 0.0, 0.4, 0.06),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, (x, y, z, e))| MotionEvent {
            t: i as f64,
            x,
            y,
            z,
            e,
            extruding: i != 0,
            line: 0,
        })
        .collect();
        let layers = split_into_layers(&events, 0.05);
        assert_eq!(
            layers.len(),
            2,
            "the z-hop should read as exactly one clean layer boundary"
        );
    }

    #[test]
    fn kept_stream_is_a_subsequence_with_nondecreasing_t() {
        // Simulate drive_firmware's t = cycle / cycles_per_second, cycle
        // strictly increasing across raw events.
        let mut dec = StepDecimator::default();
        let mut last_t: Option<f64> = None;
        for cycle in 0..2000u64 {
            let t = cycle as f64 / 1_000_000.0;
            let x = (cycle as f32) * 0.0006; // slow enough to need many steps per keep
            if dec.accept(x, 0.0, x, cycle == 1999).is_some() {
                if let Some(prev_t) = last_t {
                    assert!(t >= prev_t, "kept event t must be nondecreasing");
                }
                last_t = Some(t);
            }
        }
        assert!(last_t.is_some(), "the run should have kept at least one event");
    }
}
