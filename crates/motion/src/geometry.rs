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

/// Builds a closed-box ribbon mesh approximating deposited filament for
/// every extruding segment in `events`. Each segment contributes a top,
/// bottom and two side-wall quads, plus end caps at the start/end of each
/// extrusion run, so single-wall prints (no neighboring segments to occlude
/// the sides) still read as solid geometry rather than edge-on slivers.
pub fn build_ribbon_mesh(events: &[MotionEvent], width: f32, thickness: f32) -> MeshData {
    let mut mesh = MeshData::default();
    let half_w = width * 0.5;
    let half_t = thickness * 0.5;

    for (i, pair) in events.windows(2).enumerate() {
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

        // Unit travel direction and unit perpendicular, both in the XY plane.
        let ux = dx / len;
        let uy = dy / len;
        let px = -uy;
        let py = ux;
        let nx = px * half_w;
        let ny = py * half_w;

        let z_top = cur.z.max(prev.z) + half_t;
        let z_bot = cur.z.max(prev.z) - half_t;

        let prev_top_plus = [prev.x + nx, prev.y + ny, z_top];
        let prev_top_minus = [prev.x - nx, prev.y - ny, z_top];
        let cur_top_plus = [cur.x + nx, cur.y + ny, z_top];
        let cur_top_minus = [cur.x - nx, cur.y - ny, z_top];
        let prev_bot_plus = [prev.x + nx, prev.y + ny, z_bot];
        let prev_bot_minus = [prev.x - nx, prev.y - ny, z_bot];
        let cur_bot_plus = [cur.x + nx, cur.y + ny, z_bot];
        let cur_bot_minus = [cur.x - nx, cur.y - ny, z_bot];

        mesh.push_quad(prev_top_plus, prev_top_minus, cur_top_minus, cur_top_plus, [0.0, 0.0, 1.0]);
        mesh.push_quad(cur_bot_plus, cur_bot_minus, prev_bot_minus, prev_bot_plus, [0.0, 0.0, -1.0]);

        mesh.push_quad(prev_top_plus, cur_top_plus, cur_bot_plus, prev_bot_plus, [px, py, 0.0]);
        mesh.push_quad(cur_top_minus, prev_top_minus, prev_bot_minus, cur_bot_minus, [-px, -py, 0.0]);

        // End caps at extrusion-run boundaries. A zero-length segment mid-run
        // is skipped above without special-casing the cap here: the gap it
        // leaves is invisible since the neighboring boxes still overlap it.
        if !prev.extruding {
            mesh.push_quad(prev_top_plus, prev_bot_plus, prev_bot_minus, prev_top_minus, [-ux, -uy, 0.0]);
        }
        if events.get(i + 2).is_none_or(|next| !next.extruding) {
            mesh.push_quad(cur_top_minus, cur_bot_minus, cur_bot_plus, cur_top_plus, [ux, uy, 0.0]);
        }
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
    fn single_extruding_segment_makes_closed_box() {
        // Top, bottom, two side walls, and both end caps (isolated run) = 6 quads.
        let events = vec![ev(0.0, 0.0, 0.2, false), ev(10.0, 0.0, 0.2, true)];
        let mesh = build_ribbon_mesh(&events, 0.4, 0.2);
        assert_eq!(mesh.positions.len(), 24);
        assert_eq!(mesh.indices.len(), 36);
    }

    #[test]
    fn two_segment_run_caps_only_ends() {
        // 2 segments x 4 quads (top/bottom/2 sides) + 2 end caps (start of
        // first, end of last) = 10 quads; the interior joint gets no cap.
        let events = vec![
            ev(0.0, 0.0, 0.2, false),
            ev(10.0, 0.0, 0.2, true),
            ev(10.0, 10.0, 0.2, true),
        ];
        let mesh = build_ribbon_mesh(&events, 0.4, 0.2);
        assert_eq!(mesh.positions.len(), 40);
        assert_eq!(mesh.indices.len(), 60);
    }

    #[test]
    fn normals_are_unit_length() {
        let events = vec![ev(0.0, 0.0, 0.2, false), ev(3.0, 4.0, 0.2, true)];
        let mesh = build_ribbon_mesh(&events, 0.4, 0.2);
        for n in &mesh.normals {
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            assert!((len - 1.0).abs() < 1e-4, "normal {n:?} has length {len}");
        }
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
