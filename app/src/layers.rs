use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use motion::{build_ribbon_mesh, extend_layers, Layer, MeshData};

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
    /// Number of toolpath events `layers` was last computed from. Tracked
    /// separately from `generation` so a live session (firmware backend)
    /// that keeps appending to the same toolpath -- without bumping
    /// `generation`, which is reserved for "a new file/session was loaded"
    /// -- still gets its layer list extended incrementally (via
    /// `extend_layers`, which only rescans the still-growing tail layer
    /// rather than the whole toolpath) every frame.
    covered_len: usize,
    material: Option<Handle<StandardMaterial>>,
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
        visuals.layers.clear();
        visuals.covered_len = 0;
        visuals.generation = state.generation;
    }

    if state.toolpath.len() < visuals.covered_len {
        // Toolpath shrank without a generation bump - shouldn't happen, but
        // recover with a full reset rather than indexing out of bounds.
        for visual in visuals.visuals.drain(..).flatten() {
            commands.entity(visual.entity).despawn();
        }
        visuals.layers.clear();
        visuals.covered_len = 0;
    }
    if state.toolpath.len() != visuals.covered_len {
        // Only rescans from the start of the still-growing tail layer, not
        // the whole toolpath, so a live session appending events every
        // frame stays O(appended) per frame instead of O(toolpath length).
        extend_layers(&mut visuals.layers, &state.toolpath, LAYER_Z_THRESHOLD);
        // Finished layers keep their entities untouched below: the mesh
        // loop only rebuilds a layer whose `built_up_to` no longer matches
        // its `desired_end`, and a finished layer's desired_end (its `end`)
        // never changes once a later layer exists. The old tail layer keeps
        // its visual slot too - same `start`, so the same LayerVisual is
        // reused and simply rebuilt with the grown slice.
        let n = visuals.layers.len();
        visuals.visuals.resize_with(n, || None);
        visuals.covered_len = state.toolpath.len();
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

    let material = visuals
        .material
        .get_or_insert_with(|| {
            materials.add(StandardMaterial {
                base_color: Color::srgb(0.9, 0.55, 0.15),
                // `gcode_to_bevy` swaps axes with determinant -1 (a
                // reflection), so gcode-space CCW winding becomes CW in
                // Bevy space. Back-face culling would cull the exterior
                // faces, not the interior ones - keep this disabled.
                cull_mode: None,
                ..default()
            })
        })
        .clone();

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
                let mesh_data = build_ribbon_mesh(
                    &state.toolpath[layer.start..desired_end],
                    FILAMENT_WIDTH,
                    FILAMENT_HEIGHT,
                );
                // A leading travel-only slice (typical at the start of a
                // layer, before the first extrusion) yields an empty mesh.
                // Skip spawning until there's real geometry: handing Bevy
                // 0.19's MeshAllocator a zero-vertex mesh trips a
                // use-after-free in its allocate/copy asymmetry (upstream
                // report pending). Retried next frame as desired_end grows.
                if mesh_data.is_empty() {
                    continue;
                }
                let handle = meshes.add(to_bevy_mesh(mesh_data));
                let entity = commands
                    .spawn((
                        Mesh3d(handle.clone()),
                        MeshMaterial3d(material.clone()),
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
                    let mesh_data = build_ribbon_mesh(
                        &state.toolpath[layer.start..desired_end],
                        FILAMENT_WIDTH,
                        FILAMENT_HEIGHT,
                    );
                    // Same guard as above: never write an empty mesh into an
                    // existing tracked handle. Leaving built_up_to unset
                    // means this retries next frame with a larger slice.
                    if !mesh_data.is_empty() {
                        if let Some(mut slot) = meshes.get_mut(&visual.mesh_handle) {
                            *slot = to_bevy_mesh(mesh_data);
                        }
                        visual.built_up_to = desired_end;
                    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use motion::MotionEvent;

    fn ev(t: f64, x: f32, z: f32, extruding: bool) -> MotionEvent {
        MotionEvent {
            t,
            x,
            y: 0.0,
            z,
            e: 0.0,
            extruding,
            line: 0,
        }
    }

    /// `n` events at a fixed z, x incrementing by 1mm and t by 1s starting
    /// at (`t0`, `x0`). All but the first extrude, so consecutive events
    /// produce non-degenerate ribbon-mesh segments.
    fn layer_events(n: usize, t0: f64, x0: f32, z: f32) -> Vec<MotionEvent> {
        (0..n)
            .map(|i| ev(t0 + i as f64, x0 + i as f32, z, i != 0))
            .collect()
    }

    fn test_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.insert_resource(Assets::<Mesh>::default());
        app.insert_resource(Assets::<StandardMaterial>::default());
        app.init_resource::<PrintState>();
        app.init_resource::<LayerVisuals>();
        app.world_mut().spawn(PrintedLayerRoot);
        app.add_systems(Update, update_layer_meshes);
        app
    }

    /// Regression test for the bug fixed alongside `extend_layers`: a live
    /// session appending events to the still-growing tail layer must not
    /// despawn and rebuild every finished layer's mesh entity each frame.
    #[test]
    fn finished_layer_visual_survives_tail_growth() {
        let mut app = test_app();

        let mut events = layer_events(10, 0.0, 0.0, 0.0);
        events.extend(layer_events(10, 10.0, 0.0, 0.3));
        {
            let mut state = app.world_mut().resource_mut::<PrintState>();
            state.toolpath = events;
            state.time = 19.0;
            state.generation = 1;
        }

        app.update();

        let entity_before = {
            let visuals = app.world().resource::<LayerVisuals>();
            assert_eq!(visuals.layer_count(), 2);
            visuals.visuals[0]
                .as_ref()
                .expect("layer 0 mesh spawned")
                .entity
        };

        {
            let mut state = app.world_mut().resource_mut::<PrintState>();
            let extra = layer_events(5, 20.0, 5.0, 0.3);
            state.toolpath.extend(extra);
            state.time = 24.0;
        }

        app.update();

        let visuals = app.world().resource::<LayerVisuals>();
        assert_eq!(
            visuals.layer_count(),
            2,
            "tail growth at a constant z must not start a spurious new layer"
        );
        let entity_after = visuals.visuals[0]
            .as_ref()
            .expect("layer 0 mesh still present")
            .entity;
        assert_eq!(
            entity_before, entity_after,
            "finished layer's mesh entity must not be despawned/respawned \
             when the tail layer grows"
        );
    }
}
