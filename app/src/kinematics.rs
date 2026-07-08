use bevy::ecs::system::ParamSet;
use bevy::prelude::*;

use crate::playback::PrintState;
use crate::printer_model::NOZZLE_Z;

/// Lead of the Z screws in mm per revolution (T8 trapezoidal screw).
const SCREW_LEAD_MM: f32 = 8.0;
/// Velocity spikes from scrubbing the time slider get clamped to this.
const MAX_PLAUSIBLE_SPEED: f32 = 500.0;
/// Below this speed, direction is noise (not a real heading), so agitation
/// tracking is suspended rather than reacting to jitter while idle.
const AGITATION_MIN_SPEED: f32 = 1.0;
/// Peak-hold decay rate for `HeadVelocity.agitation`, in units/second --
/// keeps a single-frame reversal audible for roughly 0.2s instead of
/// vanishing on the very next frame.
const AGITATION_DECAY_PER_S: f32 = 5.0;

/// The X-gantry beam assembly: rides the two Z lead screws, so its world
/// height equals the gcode Z coordinate.
#[derive(Component)]
pub struct GantryRig;

/// The hotend carriage, child of [`GantryRig`]. Local origin is the nozzle
/// tip; its local X equals the gcode X coordinate.
#[derive(Component)]
pub struct CarriageRig;

/// The bed + Y carriage. Its origin is gcode (0,0,0) with the bed top
/// surface at local Y=0; gcode Y motion slides it along world Z, carrying
/// the printed layers ([`crate::printer_model::PrintedLayerRoot`]) with it.
#[derive(Component)]
pub struct BedRig;

/// A Z lead screw that only spins in place; `dir` is +1/-1 so the two
/// screws counter-rotate like a real dual-Z setup.
#[derive(Component)]
pub struct LeadScrew {
    pub dir: f32,
}

/// Relative nozzle-vs-bed speed in gcode space (mm/s), i.e. the feedrate the
/// machine is actually executing. Consumed by the DRO panel and audio.
#[derive(Resource, Default)]
pub struct HeadVelocity {
    pub mm_per_s: f32,
    /// Smoothed [0,1] direction-change intensity: ~0 for a long straight
    /// move, spikes toward 1 on a sharp turn/reversal (e.g. zig-zag infill).
    /// Peak-held with decay so a single reversal stays audible briefly
    /// rather than vanishing the next frame. Consumed by the stepper hum.
    pub agitation: f32,
    prev: Option<Vec3>,
    prev_dir: Option<Vec3>,
}

/// Decomposes the interpolated gcode position onto the bedslinger axes:
/// gcode X -> carriage, gcode Y -> bed (inverted), gcode Z -> gantry height
/// and lead-screw rotation. The nozzle stays in the fixed world-Z lane
/// `NOZZLE_Z`, so filament laid at the current Y is always exactly under it.
pub fn drive_kinematics(
    time: Res<Time>,
    state: Res<PrintState>,
    mut velocity: ResMut<HeadVelocity>,
    mut rigs: ParamSet<(
        Query<&mut Transform, With<GantryRig>>,
        Query<&mut Transform, With<CarriageRig>>,
        Query<&mut Transform, With<BedRig>>,
        Query<(&LeadScrew, &mut Transform)>,
    )>,
) {
    let (x, y, z) = state.interpolated_position().unwrap_or((0.0, 0.0, 0.0));

    let pos = Vec3::new(x, y, z);
    let dt = time.delta_secs();
    if dt > 0.0 {
        velocity.mm_per_s = match velocity.prev {
            Some(prev) => ((pos - prev).length() / dt).min(MAX_PLAUSIBLE_SPEED),
            None => 0.0,
        };
    }

    // Direction-change intensity: compares this frame's heading to the last
    // one recorded (0 = straight, 1 = a full reversal). Only tracked above a
    // minimum speed, since direction is meaningless noise near-stationary;
    // `try_normalize` guards the same zero-length case for a stopped head.
    let mut instant_agitation = 0.0;
    if velocity.mm_per_s >= AGITATION_MIN_SPEED {
        if let Some(prev) = velocity.prev {
            if let Some(dir) = (pos - prev).try_normalize() {
                if let Some(prev_dir) = velocity.prev_dir {
                    instant_agitation = ((1.0 - dir.dot(prev_dir)) / 2.0).clamp(0.0, 1.0);
                }
                velocity.prev_dir = Some(dir);
            }
        }
    }
    // Peak-hold with decay so a single sharp turn stays audible for ~0.2s
    // instead of vanishing on the very next frame.
    let decayed = velocity.agitation - dt.max(0.0) * AGITATION_DECAY_PER_S;
    velocity.agitation = instant_agitation.max(decayed).clamp(0.0, 1.0);

    velocity.prev = Some(pos);

    for mut transform in &mut rigs.p0() {
        transform.translation.y = z;
    }
    for mut transform in &mut rigs.p1() {
        transform.translation.x = x;
    }
    for mut transform in &mut rigs.p2() {
        transform.translation.z = NOZZLE_Z - y;
    }
    for (screw, mut transform) in &mut rigs.p3() {
        transform.rotation =
            Quat::from_rotation_y(screw.dir * std::f32::consts::TAU * z / SCREW_LEAD_MM);
    }
}
