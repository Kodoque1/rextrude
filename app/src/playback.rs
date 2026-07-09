use bevy::prelude::*;
use motion::{MotionEvent, ThermalSample};

/// Holds the currently loaded toolpath and the playback cursor into it.
/// Both the gcode-sim backend and (eventually) the firmware emulator backend
/// only need to populate `toolpath`; everything downstream is backend-agnostic.
#[derive(Resource, Default)]
pub struct PrintState {
    pub toolpath: Vec<MotionEvent>,
    /// Source gcode, one entry per line, for the stream panel. Empty when
    /// the toolpath has no line mapping (e.g. the firmware backend).
    pub source_lines: Vec<String>,
    /// Thermal timeline aligned with `toolpath` time. Sorted by `t`.
    pub thermal: Vec<ThermalSample>,
    pub time: f64,
    pub total_time: f64,
    pub playing: bool,
    pub speed: f32,
    pub loaded_file_name: String,
    /// Bumped every time a new toolpath is loaded, so other systems (layer
    /// mesh bookkeeping) know to throw away cached state keyed by index.
    pub generation: u64,
    /// True when the firmware-emulation backend is driving `toolpath`/`time`
    /// directly as events happen; `advance_time` (wall-clock-driven playback)
    /// is skipped in that case since there's nothing to seek ahead of.
    pub live: bool,
}

impl PrintState {
    pub fn load(
        &mut self,
        file_name: String,
        toolpath: Vec<MotionEvent>,
        source_lines: Vec<String>,
        thermal: Vec<ThermalSample>,
    ) {
        self.total_time = toolpath.last().map(|e| e.t).unwrap_or(0.0);
        self.toolpath = toolpath;
        self.source_lines = source_lines;
        self.thermal = thermal;
        self.time = 0.0;
        self.playing = true;
        self.speed = 1.0;
        self.loaded_file_name = file_name;
        self.generation += 1;
        self.live = false;
    }

    /// Index of the last event whose timestamp is <= `time`.
    pub fn current_index(&self) -> usize {
        if self.toolpath.is_empty() {
            return 0;
        }
        match self
            .toolpath
            .binary_search_by(|ev| ev.t.total_cmp(&self.time))
        {
            Ok(idx) => idx,
            Err(0) => 0,
            Err(idx) => idx - 1,
        }
    }

    /// Interpolated thermal state at a given playback time (temperature is
    /// a pure function of time, so scrubbing works transparently).
    pub fn thermal_at(&self, time: f64) -> Option<ThermalSample> {
        if self.thermal.is_empty() {
            return None;
        }
        let idx = self.thermal.partition_point(|s| s.t <= time);
        if idx == 0 {
            return Some(self.thermal[0]);
        }
        let a = self.thermal[idx - 1];
        let Some(b) = self.thermal.get(idx) else {
            return Some(a);
        };
        let span = b.t - a.t;
        let alpha = if span > f64::EPSILON {
            ((time - a.t) / span).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        Some(ThermalSample {
            t: time,
            hotend_c: a.hotend_c + (b.hotend_c - a.hotend_c) * alpha,
            hotend_target: b.hotend_target,
            bed_c: a.bed_c + (b.bed_c - a.bed_c) * alpha,
            bed_target: b.bed_target,
        })
    }

    /// Interpolated head position at the current playback time.
    pub fn interpolated_position(&self) -> Option<(f32, f32, f32)> {
        if self.toolpath.is_empty() {
            return None;
        }
        let idx = self.current_index();
        let a = self.toolpath[idx];
        let Some(b) = self.toolpath.get(idx + 1) else {
            return Some((a.x, a.y, a.z));
        };
        let span = b.t - a.t;
        let alpha = if span > f64::EPSILON {
            ((self.time - a.t) / span).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        Some((
            a.x + (b.x - a.x) * alpha,
            a.y + (b.y - a.y) * alpha,
            a.z + (b.z - a.z) * alpha,
        ))
    }
}

pub fn advance_time(time: Res<Time>, mut state: ResMut<PrintState>) {
    if state.live || !state.playing || state.toolpath.is_empty() {
        return;
    }
    state.time += time.delta_secs_f64() * state.speed as f64;
    if state.time >= state.total_time {
        state.time = state.total_time;
        state.playing = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(t: f64) -> MotionEvent {
        MotionEvent {
            t,
            x: 0.0,
            y: 0.0,
            z: 0.0,
            e: 0.0,
            extruding: false,
            line: 0,
        }
    }

    /// Regression test: `current_index` used `partial_cmp(...).unwrap()`,
    /// which panics if any event timestamp is `NaN` (`partial_cmp` returns
    /// `None` for NaN comparisons, unlike infinity which is still totally
    /// ordered and comparable). `total_cmp` gives NaN a well-defined slot in
    /// the order instead of panicking.
    #[test]
    fn current_index_does_not_panic_on_nan_event_time() {
        let mut state = PrintState {
            toolpath: vec![ev(0.0), ev(1.0), ev(f64::NAN)],
            ..Default::default()
        };
        state.time = 0.5;
        let _ = state.current_index(); // must not panic

        state.time = f64::NAN;
        let _ = state.current_index(); // must not panic
    }

    /// Infinite (but non-NaN) event timestamps are a realistic hostile-file
    /// outcome (see gcode-sim's huge-move handling) and were never the
    /// source of the panic, but confirm they still resolve sensibly.
    #[test]
    fn current_index_handles_infinite_event_time() {
        let mut state = PrintState {
            toolpath: vec![ev(0.0), ev(1.0), ev(f64::INFINITY)],
            ..Default::default()
        };
        state.time = 0.5;
        assert_eq!(state.current_index(), 0);
        state.time = f64::INFINITY;
        assert_eq!(state.current_index(), 2);
    }
}
