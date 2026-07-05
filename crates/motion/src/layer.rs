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
        }
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
}
