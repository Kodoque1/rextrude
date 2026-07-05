use bevy::input::mouse::{MouseMotion, MouseWheel};
use bevy::prelude::*;

#[derive(Component)]
pub struct OrbitCamera {
    pub focus: Vec3,
    pub radius: f32,
    pub yaw: f32,
    pub pitch: f32,
}

impl Default for OrbitCamera {
    fn default() -> Self {
        Self {
            focus: Vec3::new(110.0, 40.0, 110.0),
            radius: 380.0,
            // Start on the operator side (machine -Y): hotend, decals and
            // the bed's hazard trim face this way; the X-beam sits behind
            // the carriage like on a real i3.
            yaw: std::f32::consts::PI - 1.05,
            pitch: 0.55,
        }
    }
}

fn camera_transform(orbit: &OrbitCamera) -> Transform {
    let rot = Quat::from_euler(EulerRot::YXZ, orbit.yaw, -orbit.pitch, 0.0);
    let pos = orbit.focus + rot * Vec3::new(0.0, 0.0, orbit.radius);
    Transform::from_translation(pos).looking_at(orbit.focus, Vec3::Y)
}

pub fn setup_camera(mut commands: Commands, canvas: Res<crate::psx::PsxCanvasImage>) {
    let orbit = OrbitCamera::default();
    let transform = camera_transform(&orbit);
    commands.spawn((
        Camera3d::default(),
        transform,
        orbit,
        crate::psx::psx_camera_3d_bundle(&canvas),
    ));
}

/// Right-mouse-drag orbits, scroll wheel zooms. Left click / drag is left
/// free for egui widgets so the two don't fight over pointer input.
pub fn orbit_camera(
    mut mouse_motion: MessageReader<MouseMotion>,
    mut mouse_wheel: MessageReader<MouseWheel>,
    mouse_button: Res<ButtonInput<MouseButton>>,
    mut query: Query<(&mut Transform, &mut OrbitCamera)>,
) {
    let dragging = mouse_button.pressed(MouseButton::Right);
    let mut delta = Vec2::ZERO;
    for ev in mouse_motion.read() {
        delta += ev.delta;
    }
    let mut scroll = 0.0;
    for ev in mouse_wheel.read() {
        scroll += ev.y;
    }

    if delta == Vec2::ZERO && scroll == 0.0 {
        return;
    }

    for (mut transform, mut orbit) in &mut query {
        if dragging {
            orbit.yaw -= delta.x * 0.005;
            orbit.pitch = (orbit.pitch + delta.y * 0.005).clamp(-1.5, 1.5);
        }
        if scroll != 0.0 {
            orbit.radius = (orbit.radius - scroll * 20.0).clamp(50.0, 1200.0);
        }
        *transform = camera_transform(&orbit);
    }
}
