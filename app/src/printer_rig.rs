use bevy::gltf::GltfAssetLabel;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;

use crate::kinematics::{BedRig, CarriageRig, GantryRig, LeadScrew};
use crate::printer_model::Placeholder;

/// Which rig a named glb node belongs to. `Static` nodes stay where the
/// scene put them but still count toward discovery completion.
#[derive(Clone, Copy)]
enum RigTarget {
    Gantry,
    Carriage,
    Bed,
    Screw { dir: f32 },
    Static,
}

/// glb node names (authored in tools/gen_printer_assets.py) still waiting
/// to be discovered. Once empty, the primitive placeholders are despawned.
#[derive(Resource)]
pub struct PendingRigParts(HashMap<&'static str, RigTarget>);

impl Default for PendingRigParts {
    fn default() -> Self {
        Self(HashMap::from_iter([
            ("Frame_Static", RigTarget::Static),
            ("Gantry_X", RigTarget::Gantry),
            ("Carriage_X", RigTarget::Carriage),
            ("Bed_Y", RigTarget::Bed),
            ("LeadScrew_L", RigTarget::Screw { dir: 1.0 }),
            ("LeadScrew_R", RigTarget::Screw { dir: -1.0 }),
        ]))
    }
}

pub fn spawn_printer_scene(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.spawn((
        WorldAssetRoot(
            asset_server.load(GltfAssetLabel::Scene(0).from_asset("models/printer.glb")),
        ),
        Transform::default(),
        Visibility::default(),
    ));
}

/// The new scene system has no "scene ready" event, so newly spawned glb
/// nodes are picked up by name as they appear. Moving parts are reparented
/// under their app-owned rig entity (which the kinematics system drives);
/// lead screws keep their glb translation and only get spun in place.
pub fn discover_rig_parts(
    mut commands: Commands,
    mut pending: ResMut<PendingRigParts>,
    named: Query<(Entity, &Name), Added<Name>>,
    gantry: Query<Entity, With<GantryRig>>,
    carriage: Query<Entity, With<CarriageRig>>,
    bed: Query<Entity, With<BedRig>>,
    placeholders: Query<Entity, With<Placeholder>>,
) {
    if pending.0.is_empty() {
        return;
    }
    for (entity, name) in &named {
        let Some(&target) = pending.0.get(name.as_str()) else {
            continue;
        };
        let reparent_to = match target {
            RigTarget::Gantry => gantry.single().ok(),
            RigTarget::Carriage => carriage.single().ok(),
            RigTarget::Bed => bed.single().ok(),
            RigTarget::Screw { dir } => {
                commands.entity(entity).insert(LeadScrew { dir });
                None
            }
            RigTarget::Static => None,
        };
        if let Some(parent) = reparent_to {
            commands
                .entity(entity)
                .insert(Transform::IDENTITY)
                .insert(ChildOf(parent));
        }
        pending.0.remove(name.as_str());
    }
    if pending.0.is_empty() {
        for entity in &placeholders {
            commands.entity(entity).despawn();
        }
        info!("printer.glb parts discovered; placeholders despawned");
    }
}
