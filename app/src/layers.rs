use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use motion::{build_ribbon_mesh, split_into_layers, Layer, MeshData};

use crate::coords::gcode_to_bevy;
use crate::playback::PrintState;
use crate::printer_model::PrintedLayerRoot;

/// Z jump (mm) that marks the start of a new layer.
const LAYER_Z_THRESHOLD: f32 = 0.05;
/// Cosmetic filament cross-section, typical of a 0.4mm nozzle.
const FILAMENT_WIDTH: f32 = 0.45;
const FILAMENT_HEIGHT: f32 = 0.2;

struct LayerVisual {
    entity: Entity,
    mesh_handle: Handle<Mesh>,
    built_up_to: usize,
}

/// Tracks one mesh entity per print layer. Only the currently-active layer
/// (the one the playhead is inside) is rebuilt each frame; finished layers
/// are left untouched, and layers the playhead has scrubbed back before are
/// despawned and lazily rebuilt if played forward again.
#[derive(Resource, Default)]
pub struct LayerVisuals {
    layers: Vec<Layer>,
    visuals: Vec<Option<LayerVisual>>,
    generation: u64,
}

impl LayerVisuals {
    pub fn layer_count(&self) -> usize {
        self.layers.len()
    }

    pub fn layer_containing(&self, event_index: usize) -> Option<usize> {
        self.layers
            .iter()
            .position(|l| event_index >= l.start && event_index < l.end)
    }
}

fn to_bevy_mesh(data: MeshData) -> Mesh {
    let positions: Vec<[f32; 3]> = data
        .positions
        .iter()
        .map(|&[x, y, z]| gcode_to_bevy(x, y, z).to_array())
        .collect();
    let normals: Vec<[f32; 3]> = data
        .normals
        .iter()
        .map(|&[x, y, z]| gcode_to_bevy(x, y, z).to_array())
        .collect();

    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_indices(Indices::U32(data.indices))
}

pub fn update_layer_meshes(
    mut commands: Commands,
    state: Res<PrintState>,
    mut visuals: ResMut<LayerVisuals>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    root_query: Query<Entity, With<PrintedLayerRoot>>,
) {
    if state.toolpath.is_empty() {
        return;
    }

    if visuals.generation != state.generation {
        for visual in visuals.visuals.drain(..).flatten() {
            commands.entity(visual.entity).despawn();
        }
        visuals.layers = split_into_layers(&state.toolpath, LAYER_Z_THRESHOLD);
        visuals.visuals = visuals.layers.iter().map(|_| None).collect();
        visuals.generation = state.generation;
    }

    if visuals.layers.is_empty() {
        return;
    }

    let idx = state.current_index();
    let current_layer = visuals
        .layers
        .iter()
        .position(|l| idx >= l.start && idx < l.end)
        .unwrap_or(visuals.layers.len() - 1);

    let Ok(root) = root_query.single() else {
        return;
    };

    for i in 0..visuals.layers.len() {
        let layer = visuals.layers[i];
        let desired_end = match i.cmp(&current_layer) {
            std::cmp::Ordering::Less => layer.end,
            std::cmp::Ordering::Equal => (idx + 1).min(layer.end),
            std::cmp::Ordering::Greater => layer.start,
        };
        let needs_mesh = desired_end > layer.start + 1;

        match (visuals.visuals[i].as_mut(), needs_mesh) {
            (None, true) => {
                let mesh = to_bevy_mesh(build_ribbon_mesh(
                    &state.toolpath[layer.start..desired_end],
                    FILAMENT_WIDTH,
                    FILAMENT_HEIGHT,
                ));
                let handle = meshes.add(mesh);
                let material = materials.add(StandardMaterial {
                    base_color: Color::srgb(0.9, 0.55, 0.15),
                    cull_mode: None,
                    ..default()
                });
                let entity = commands
                    .spawn((
                        Mesh3d(handle.clone()),
                        MeshMaterial3d(material),
                        Transform::default(),
                    ))
                    .id();
                commands.entity(root).add_child(entity);
                visuals.visuals[i] = Some(LayerVisual {
                    entity,
                    mesh_handle: handle,
                    built_up_to: desired_end,
                });
            }
            (Some(visual), true) => {
                if visual.built_up_to != desired_end {
                    let mesh = to_bevy_mesh(build_ribbon_mesh(
                        &state.toolpath[layer.start..desired_end],
                        FILAMENT_WIDTH,
                        FILAMENT_HEIGHT,
                    ));
                    if let Some(mut slot) = meshes.get_mut(&visual.mesh_handle) {
                        *slot = mesh;
                    }
                    visual.built_up_to = desired_end;
                }
            }
            (Some(visual), false) => {
                commands.entity(visual.entity).despawn();
                visuals.visuals[i] = None;
            }
            (None, false) => {}
        }
    }
}
