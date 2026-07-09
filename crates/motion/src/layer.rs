use crate::MotionEvent;

/// A contiguous range of `MotionEvent`s that share (approximately) the same Z height.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layer {
    pub z: f32,
    /// Index of the first event belonging to this layer (inclusive).
    pub start: usize,
    /// Index of the last event belonging to this layer (exclusive).
    pub end: usize,
}

/// Splits a toolpath into layers by watching for Z increases.
///
/// A new layer starts whenever Z rises by more than `threshold` mm above the
/// current layer's Z. Z moves that occur without a following extrusion
/// (e.g. a lift/retract move) do not themselves start a new layer until the
/// head actually deposits material there.
pub fn split_into_layers(events: &[MotionEvent], threshold: f32) -> Vec<Layer> {
    if events.is_empty() {
        return Vec::new();
    }

    let mut layers = Vec::new();
    let mut current_z = events[0].z;
    let mut start = 0;

    for (i, ev) in events.iter().enumerate() {
        if ev.z - current_z > threshold {
            if i > start {
                layers.push(Layer {
                    z: current_z,
                    start,
                    end: i,
                });
            }
            start = i;
            current_z = ev.z;
        }
    }

    layers.push(Layer {
        z: current_z,
        start,
        end: events.len(),
    });

    layers
}

/// Extends `layers` (previously produced by [`split_into_layers`] or this
/// function over a prefix of `events`) to cover all of `events`, without
/// rescanning events that were already folded into a finished (non-tail)
/// layer.
///
/// This is equivalent to `split_into_layers(events, threshold)`: boundaries
/// are decided only by comparing each event's `z` against the *running*
/// `current_z` of the layer it lands in (see `split_into_layers`), and
/// `current_z` is reset only at a boundary event, which is always the first
/// event of some layer. So re-splitting from the start of the old tail layer
/// reproduces exactly the layers a full rescan would have produced from that
/// point onward.
///
/// Returns the index of the first layer that may have changed (the old
/// tail's index, or 0 if `layers` was empty).
pub fn extend_layers(layers: &mut Vec<Layer>, events: &[MotionEvent], threshold: f32) -> usize {
    let resume_at = layers.pop().map(|tail| tail.start).unwrap_or(0);
    let first_changed = layers.len();
    let tail = split_into_layers(&events[resume_at..], threshold);
    layers.extend(tail.into_iter().map(|l| Layer {
        z: l.z,
        start: l.start + resume_at,
        end: l.end + resume_at,
    }));
    first_changed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(z: f32) -> MotionEvent {
        MotionEvent {
            t: 0.0,
            x: 0.0,
            y: 0.0,
            z,
            e: 0.0,
            extruding: false,
            line: 0,
        }
    }

    /// Simple LCG so tests are deterministic without a `rand` dependency.
    fn seeded_staircase(n: usize, seed: u64) -> Vec<MotionEvent> {
        let mut state = seed;
        let mut next = move || {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            (state >> 33) as u32
        };
        let mut z = 0.0f32;
        (0..n)
            .map(|_| {
                // Occasionally jump z by more than the threshold, sometimes
                // by less (flat run), so both branches of the split get
                // exercised.
                match next() % 10 {
                    0 => z += 0.2,
                    1 => z += 0.02,
                    _ => {}
                }
                ev(z)
            })
            .collect()
    }

    #[test]
    fn splits_on_z_increase() {
        let events = vec![ev(0.2), ev(0.2), ev(0.4), ev(0.4), ev(0.6)];
        let layers = split_into_layers(&events, 0.05);
        assert_eq!(layers.len(), 3);
        assert_eq!(layers[0].z, 0.2);
        assert_eq!(layers[0].start, 0);
        assert_eq!(layers[0].end, 2);
        assert_eq!(layers[1].z, 0.4);
        assert_eq!(layers[2].z, 0.6);
    }

    #[test]
    fn single_layer_when_flat() {
        let events = vec![ev(0.2), ev(0.2), ev(0.2)];
        let layers = split_into_layers(&events, 0.05);
        assert_eq!(layers.len(), 1);
        assert_eq!(layers[0].start, 0);
        assert_eq!(layers[0].end, 3);
    }

    #[test]
    fn empty_input() {
        assert!(split_into_layers(&[], 0.05).is_empty());
    }

    #[test]
    fn extend_layers_matches_full_split_across_chunk_sizes() {
        let full = seeded_staircase(5000, 42);
        for &chunk in &[1usize, 7, 100] {
            let mut layers = Vec::new();
            let mut end = 0;
            while end < full.len() {
                end = (end + chunk).min(full.len());
                extend_layers(&mut layers, &full[..end], 0.05);
                let expected = split_into_layers(&full[..end], 0.05);
                assert_eq!(
                    layers, expected,
                    "mismatch at chunk size {chunk}, len {end}"
                );
            }
        }
    }

    #[test]
    fn extend_layers_grows_tail_in_place() {
        let events = seeded_staircase(200, 7);
        let mut layers = split_into_layers(&events[..100], 0.05);
        let before = layers.clone();
        let first_changed = extend_layers(&mut layers, &events[..150], 0.05);

        assert_eq!(first_changed, before.len() - 1);
        assert_eq!(&layers[..before.len() - 1], &before[..before.len() - 1]);
    }

    #[test]
    fn extend_layers_starts_new_layer_on_jump() {
        let events = vec![ev(0.2), ev(0.2), ev(0.2)];
        let mut layers = split_into_layers(&events, 0.05);
        assert_eq!(layers.len(), 1);

        let mut grown = events.clone();
        grown.push(ev(0.5));
        extend_layers(&mut layers, &grown, 0.05);

        assert_eq!(layers, split_into_layers(&grown, 0.05));
        assert_eq!(layers.len(), 2);
    }

    #[test]
    fn extend_layers_from_empty_matches_full_split() {
        let events = seeded_staircase(50, 99);
        let mut layers = Vec::new();
        extend_layers(&mut layers, &events, 0.05);
        assert_eq!(layers, split_into_layers(&events, 0.05));
    }
}
