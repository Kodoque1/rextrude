//! Guards against reintroducing the "recompute the whole toolpath every
//! frame" regression: a live session appending events to `LayerVisuals`
//! must cost O(appended events) per frame, not O(total toolpath length).
//!
//! Ignored by default (only run in release, explicitly, in CI's `perf` job)
//! since it's a wall-clock measurement, not a deterministic unit test.

use motion::{extend_layers, MotionEvent};
use std::time::{Duration, Instant};

fn synth_toolpath(n: usize, seed: u64) -> Vec<MotionEvent> {
    let mut state = seed;
    let mut next = move || {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        (state >> 33) as u32
    };
    let mut z = 0.0f32;
    (0..n)
        .map(|i| {
            if next() % 500 == 0 {
                z += 0.2;
            }
            MotionEvent {
                t: i as f64,
                x: (i as f32 * 0.4) % 200.0,
                y: (i as f32 * 0.7) % 200.0,
                z,
                e: i as f32 * 0.01,
                extruding: i % 7 != 0,
                line: 0,
            }
        })
        .collect()
}

#[test]
#[ignore]
fn per_frame_extend_cost_stays_flat_as_toolpath_grows() {
    const TOTAL_EVENTS: usize = 2_000_000;
    const CHUNK: usize = 1_000;
    const FRAMES: usize = TOTAL_EVENTS / CHUNK;
    const BUCKET: usize = FRAMES / 10;

    let events = synth_toolpath(TOTAL_EVENTS, 42);
    let mut layers: Vec<motion::Layer> = Vec::new();
    let mut frame_durations = Vec::with_capacity(FRAMES);

    let start = Instant::now();
    for frame in 0..FRAMES {
        let end = ((frame + 1) * CHUNK).min(events.len());
        let t0 = Instant::now();
        extend_layers(&mut layers, &events[..end], 0.05);
        frame_durations.push(t0.elapsed());
    }
    let total = start.elapsed();

    let early: Duration = frame_durations[..BUCKET].iter().sum();
    let late: Duration = frame_durations[frame_durations.len() - BUCKET..]
        .iter()
        .sum();

    assert!(
        total < Duration::from_secs(30),
        "total incremental-extend time {total:?} exceeds the generous \
         absolute ceiling; something is scaling far worse than expected"
    );
    assert!(
        late < early * 8,
        "per-frame extend_layers cost grew from {early:?} (first {BUCKET} \
         frames) to {late:?} (last {BUCKET} frames) as the toolpath grew to \
         {TOTAL_EVENTS} events - this smells like a reintroduced full \
         rescan-per-frame regression instead of O(appended) incremental work"
    );
}
