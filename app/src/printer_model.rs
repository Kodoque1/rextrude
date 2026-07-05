use bevy::prelude::*;

pub const BED_SIZE: f32 = 220.0;
pub const BED_THICKNESS: f32 = 4.0;
pub const FRAME_HEIGHT: f32 = 260.0;
pub const FRAME_MARGIN: f32 = 20.0;

/// Marker for the entity whose transform tracks the interpolated nozzle
/// position every frame (see `playback::update_head_transform`).
#[derive(Component)]
pub struct PrintHead;

/// Parent entity that all per-layer filament meshes are spawned under.
#[derive(Component)]
pub struct PrintedLayerRoot;

pub fn setup_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        DirectionalLight {
            illuminance: 7000.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(150.0, 300.0, -50.0).looking_at(Vec3::new(110.0, 0.0, 110.0), Vec3::Y),
    ));
    commands.insert_resource(GlobalAmbientLight {
        color: Color::WHITE,
        brightness: 400.0,
        ..default()
    });

    // Base plate for visual context.
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(340.0, 340.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.10, 0.10, 0.12),
            ..default()
        })),
        Transform::from_xyz(BED_SIZE / 2.0, -BED_THICKNESS - 0.5, BED_SIZE / 2.0),
    ));

    // Print bed.
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(BED_SIZE, BED_THICKNESS, BED_SIZE))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.25, 0.5, 0.35),
            perceptual_roughness: 0.9,
            ..default()
        })),
        Transform::from_xyz(BED_SIZE / 2.0, -BED_THICKNESS / 2.0, BED_SIZE / 2.0),
    ));

    // Decorative frame posts, purely for visual context (not kinematically
    // linked to the toolpath — only the head assembly moves).
    let post_mesh = meshes.add(Cylinder::new(4.0, FRAME_HEIGHT));
    let post_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.55, 0.55, 0.6),
        ..default()
    });
    for (dx, dz) in [
        (-FRAME_MARGIN, -FRAME_MARGIN),
        (BED_SIZE + FRAME_MARGIN, -FRAME_MARGIN),
        (-FRAME_MARGIN, BED_SIZE + FRAME_MARGIN),
        (BED_SIZE + FRAME_MARGIN, BED_SIZE + FRAME_MARGIN),
    ] {
        commands.spawn((
            Mesh3d(post_mesh.clone()),
            MeshMaterial3d(post_mat.clone()),
            Transform::from_xyz(dx, FRAME_HEIGHT / 2.0, dz),
        ));
    }

    // Print head assembly: a carriage block with a nozzle cone whose tip is
    // the entity's local origin, so `PrintHead`'s translation always equals
    // the nozzle's gcode (x, y, z).
    let carriage_mesh = meshes.add(Cuboid::new(28.0, 18.0, 28.0));
    let carriage_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.8, 0.2, 0.2),
        ..default()
    });
    let nozzle_mesh = meshes.add(Cone::new(5.0, 12.0));
    let nozzle_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.85, 0.75, 0.2),
        ..default()
    });

    commands
        .spawn((
            PrintHead,
            Transform::from_xyz(0.0, 0.0, 0.0),
            Visibility::default(),
        ))
        .with_children(|parent| {
            parent.spawn((
                Mesh3d(carriage_mesh),
                MeshMaterial3d(carriage_mat),
                Transform::from_xyz(0.0, 24.0, 0.0),
            ));
            parent.spawn((
                Mesh3d(nozzle_mesh),
                MeshMaterial3d(nozzle_mat),
                Transform::from_xyz(0.0, 6.0, 0.0),
            ));
        });

    commands.spawn((
        PrintedLayerRoot,
        Transform::default(),
        Visibility::default(),
    ));
}
