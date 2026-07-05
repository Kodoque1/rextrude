use bevy::prelude::*;

use crate::kinematics::{BedRig, CarriageRig, GantryRig, LeadScrew};

pub const BED_SIZE: f32 = 220.0;
pub const BED_THICKNESS: f32 = 4.0;
pub const FRAME_HEIGHT: f32 = 260.0;
/// Fixed world-Z lane the nozzle lives in. The bed slides under it to realize
/// gcode Y motion (i3-style bedslinger), so the machine is symmetric around
/// this plane: bed local origin sits at world Z = NOZZLE_Z - gcode_y.
pub const NOZZLE_Z: f32 = BED_SIZE / 2.0;

/// X positions of the two frame uprights / lead screws, just outside travel.
const FRAME_X_LEFT: f32 = -45.0;
const FRAME_X_RIGHT: f32 = BED_SIZE + 45.0;
const SCREW_X_LEFT: f32 = -22.0;
const SCREW_X_RIGHT: f32 = BED_SIZE + 22.0;

/// Parent entity that all per-layer filament meshes are spawned under.
/// A child of [`BedRig`], so the printed object rides the moving bed.
#[derive(Component)]
pub struct PrintedLayerRoot;

/// Placeholder primitive meshes standing in for the Blender-authored glb
/// parts; despawned once the real parts are discovered and reparented.
#[derive(Component)]
pub struct Placeholder;

pub fn setup_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        DirectionalLight {
            color: Color::srgb(0.85, 1.0, 0.88),
            illuminance: 5500.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(150.0, 300.0, -50.0).looking_at(Vec3::new(110.0, 0.0, 110.0), Vec3::Y),
    ));
    commands.insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.75, 1.0, 0.82),
        brightness: 300.0,
        ..default()
    });

    // Base plate, sized for the full bed travel envelope (world Z -110..330).
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(380.0, 560.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.10, 0.10, 0.12),
            ..default()
        })),
        Transform::from_xyz(BED_SIZE / 2.0, -14.0, NOZZLE_Z),
    ));

    let steel = materials.add(StandardMaterial {
        base_color: Color::srgb(0.55, 0.55, 0.6),
        ..default()
    });
    let dark_steel = materials.add(StandardMaterial {
        base_color: Color::srgb(0.3, 0.3, 0.35),
        ..default()
    });

    // Two Y rails the bed carriage slides on, spanning the travel envelope.
    let rail_mesh = meshes.add(Cuboid::new(12.0, 6.0, 470.0));
    for x in [60.0, 160.0] {
        commands.spawn((
            Placeholder,
            Mesh3d(rail_mesh.clone()),
            MeshMaterial3d(dark_steel.clone()),
            Transform::from_xyz(x, -10.0, NOZZLE_Z),
        ));
    }

    // Two frame uprights in the nozzle lane, carrying the gantry.
    let post_mesh = meshes.add(Cuboid::new(14.0, FRAME_HEIGHT, 14.0));
    for x in [FRAME_X_LEFT, FRAME_X_RIGHT] {
        commands.spawn((
            Placeholder,
            Mesh3d(post_mesh.clone()),
            MeshMaterial3d(steel.clone()),
            Transform::from_xyz(x, FRAME_HEIGHT / 2.0 - 13.0, NOZZLE_Z),
        ));
    }

    // Lead screws: spin in place (rotation driven by `drive_kinematics`);
    // the small flag cube makes the rotation visible on placeholder shapes.
    let screw_mesh = meshes.add(Cylinder::new(3.0, FRAME_HEIGHT - 30.0));
    let flag_mesh = meshes.add(Cuboid::new(9.0, 3.0, 3.0));
    for (x, dir) in [(SCREW_X_LEFT, 1.0), (SCREW_X_RIGHT, -1.0)] {
        commands
            .spawn((
                LeadScrew { dir },
                Placeholder,
                Transform::from_xyz(x, (FRAME_HEIGHT - 30.0) / 2.0, NOZZLE_Z),
                Visibility::default(),
            ))
            .with_children(|parent| {
                parent.spawn((
                    Mesh3d(screw_mesh.clone()),
                    MeshMaterial3d(steel.clone()),
                    Transform::default(),
                ));
                parent.spawn((
                    Mesh3d(flag_mesh.clone()),
                    MeshMaterial3d(dark_steel.clone()),
                    Transform::from_xyz(5.0, 0.0, 0.0),
                ));
            });
    }

    // Gantry rig: world Y == gcode Z. The beam is a child; the carriage rig
    // hangs from it with its local origin at the nozzle tip so its local X
    // is exactly the gcode X.
    let beam_mesh = meshes.add(Cuboid::new(FRAME_X_RIGHT - FRAME_X_LEFT + 30.0, 16.0, 16.0));
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
        .spawn((GantryRig, Transform::default(), Visibility::default()))
        .with_children(|gantry| {
            gantry.spawn((
                Placeholder,
                Mesh3d(beam_mesh),
                MeshMaterial3d(steel.clone()),
                Transform::from_xyz(BED_SIZE / 2.0, 42.0, NOZZLE_Z),
            ));
            gantry
                .spawn((
                    CarriageRig,
                    Transform::from_xyz(0.0, 0.0, NOZZLE_Z),
                    Visibility::default(),
                ))
                .with_children(|carriage| {
                    carriage.spawn((
                        Placeholder,
                        Mesh3d(carriage_mesh),
                        MeshMaterial3d(carriage_mat),
                        Transform::from_xyz(0.0, 24.0, 0.0),
                    ));
                    carriage.spawn((
                        Placeholder,
                        Mesh3d(nozzle_mesh),
                        MeshMaterial3d(nozzle_mat),
                        Transform::from_xyz(0.0, 6.0, 0.0),
                    ));
                });
        });

    // Bed rig: origin = gcode (0,0,0), bed top surface at local Y=0.
    // Starts at home (gcode y = 0 -> world z = NOZZLE_Z).
    let bed_mesh = meshes.add(Cuboid::new(BED_SIZE, BED_THICKNESS, BED_SIZE));
    let bed_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.25, 0.5, 0.35),
        perceptual_roughness: 0.9,
        ..default()
    });
    let bed_carriage_mesh = meshes.add(Cuboid::new(150.0, 6.0, 240.0));

    commands
        .spawn((
            BedRig,
            Transform::from_xyz(0.0, 0.0, NOZZLE_Z),
            Visibility::default(),
        ))
        .with_children(|bed| {
            bed.spawn((
                Placeholder,
                Mesh3d(bed_mesh),
                MeshMaterial3d(bed_mat),
                Transform::from_xyz(BED_SIZE / 2.0, -BED_THICKNESS / 2.0, BED_SIZE / 2.0),
            ));
            bed.spawn((
                Placeholder,
                Mesh3d(bed_carriage_mesh),
                MeshMaterial3d(dark_steel),
                Transform::from_xyz(BED_SIZE / 2.0, -BED_THICKNESS - 3.0, BED_SIZE / 2.0),
            ));
            bed.spawn((PrintedLayerRoot, Transform::default(), Visibility::default()));
        });
}
