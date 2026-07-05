use bevy::ecs::system::ParamSet;
use bevy::prelude::*;

use crate::playback::PrintState;
use crate::printer_model::NOZZLE_Z;

/// Lead of the Z screws in mm per revolution (T8 trapezoidal screw).
const SCREW_LEAD_MM: f32 = 8.0;
/// Velocity spikes from scrubbing the time slider get clamped to this.
const MAX_PLAUSIBLE_SPEED: f32 = 500.0;

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
    prev: Option<Vec3>,
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
