use crate::MotionEvent;

/// Renderer-agnostic triangle mesh data. The `app` crate converts this into
/// whatever mesh type the active graphics backend expects.
#[derive(Debug, Clone, Default)]
pub struct MeshData {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
}

impl MeshData {
    fn push_quad(&mut self, a: [f32; 3], b: [f32; 3], c: [f32; 3], d: [f32; 3], normal: [f32; 3]) {
        let base = self.positions.len() as u32;
        self.positions.extend_from_slice(&[a, b, c, d]);
        self.normals.extend_from_slice(&[normal, normal, normal, normal]);
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }
}

/// Builds a flat top/bottom ribbon mesh approximating deposited filament for
/// every extruding segment in `events`. Each segment contributes a top quad
/// and a bottom quad (4 triangles) rather than a full watertight box: side
/// walls are skipped since consecutive segments occlude them, which keeps
/// the mesh cheap enough to rebuild per layer during playback.
pub fn build_ribbon_mesh(events: &[MotionEvent], width: f32, thickness: f32) -> MeshData {
    let mut mesh = MeshData::default();
    let half_w = width * 0.5;
    let half_t = thickness * 0.5;

    for pair in events.windows(2) {
        let (prev, cur) = (pair[0], pair[1]);
        if !cur.extruding {
            continue;
        }

        let dx = cur.x - prev.x;
        let dy = cur.y - prev.y;
        let len = (dx * dx + dy * dy).sqrt();
        if len < f32::EPSILON {
            continue;
        }

        // Perpendicular to the travel direction, in the XY plane.
        let nx = -dy / len * half_w;
        let ny = dx / len * half_w;

        let z_top = cur.z.max(prev.z) + half_t;
        let z_bot = cur.z.max(prev.z) - half_t;

        let a = [prev.x + nx, prev.y + ny, z_top];
        let b = [prev.x - nx, prev.y - ny, z_top];
        let c = [cur.x - nx, cur.y - ny, z_top];
        let d = [cur.x + nx, cur.y + ny, z_top];
        mesh.push_quad(a, b, c, d, [0.0, 0.0, 1.0]);

        let a2 = [prev.x + nx, prev.y + ny, z_bot];
        let b2 = [prev.x - nx, prev.y - ny, z_bot];
        let c2 = [cur.x - nx, cur.y - ny, z_bot];
        let d2 = [cur.x + nx, cur.y + ny, z_bot];
        mesh.push_quad(d2, c2, b2, a2, [0.0, 0.0, -1.0]);
    }

    mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(x: f32, y: f32, z: f32, extruding: bool) -> MotionEvent {
        MotionEvent {
            t: 0.0,
            x,
            y,
            z,
            e: if extruding { 1.0 } else { 0.0 },
            extruding,
            line: 0,
        }
    }

    #[test]
    fn single_extruding_segment_makes_two_quads() {
        let events = vec![ev(0.0, 0.0, 0.2, false), ev(10.0, 0.0, 0.2, true)];
        let mesh = build_ribbon_mesh(&events, 0.4, 0.2);
        assert_eq!(mesh.positions.len(), 8);
        assert_eq!(mesh.indices.len(), 12);
    }

    #[test]
    fn travel_move_produces_nothing() {
        let events = vec![ev(0.0, 0.0, 0.2, false), ev(10.0, 0.0, 0.2, false)];
        let mesh = build_ribbon_mesh(&events, 0.4, 0.2);
        assert!(mesh.is_empty());
    }

    #[test]
    fn zero_length_segment_is_skipped() {
        let events = vec![ev(5.0, 5.0, 0.2, false), ev(5.0, 5.0, 0.2, true)];
        let mesh = build_ribbon_mesh(&events, 0.4, 0.2);
        assert!(mesh.is_empty());
    }
}
