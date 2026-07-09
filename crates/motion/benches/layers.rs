use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use motion::{build_ribbon_mesh, extend_layers, split_into_layers, MotionEvent};

/// Deterministic synthetic toolpath: an XY-moving, Z-stepping staircase, the
/// same shape a real print's layer boundaries look like.
fn synth_toolpath(n: usize, seed: u64) -> Vec<MotionEvent> {
    let mut state = seed;
    let mut next = move || {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        (state >> 33) as u32
    };
    let mut z = 0.0f32;
    (0..n)
        .map(|i| {
            // Roughly one Z step every 200 events: ~1000 layers at n=200_000.
            if next() % 200 == 0 {
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

fn bench_split_full(c: &mut Criterion) {
    let events = synth_toolpath(200_000, 1);
    c.bench_function("split_into_layers/200k_events", |b| {
        b.iter(|| split_into_layers(&events, 0.05));
    });
}

fn bench_extend_amortized(c: &mut Criterion) {
    let events = synth_toolpath(200_000, 2);
    let base = 190_000;
    let base_layers = split_into_layers(&events[..base], 0.05);
    c.bench_function("extend_layers/amortized_100_event_chunk", |b| {
        b.iter_batched(
            || base_layers.clone(),
            |mut layers| extend_layers(&mut layers, &events[..base + 100], 0.05),
            BatchSize::SmallInput,
        );
    });
}

fn bench_ribbon_mesh(c: &mut Criterion) {
    let events = synth_toolpath(1_000, 3);
    c.bench_function("build_ribbon_mesh/1k_events", |b| {
        b.iter(|| build_ribbon_mesh(&events, 0.45, 0.2));
    });
}

criterion_group!(
    benches,
    bench_split_full,
    bench_extend_amortized,
    bench_ribbon_mesh
);
criterion_main!(benches);
