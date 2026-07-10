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

/// Max toolpath events fed through `build_ribbon_mesh` per frame, summed
/// across every layer touched that frame. Without this, a fast jump across a
/// huge toolpath (a multi-million-event firmware or simulation session)
/// would build every intervening layer's mesh in a single frame -- on a
/// 2.8M-event file that stalls long enough for the browser to queue a burst
/// of pointer input, which then replays as an apparent "the timeline jumped
/// back to the start". Calibrated against the
/// `build_ribbon_mesh/50k_events_frame_budget` bench
/// (crates/motion/benches/layers.rs), which measured ~13.9ms native/release
/// for 50k events -- too close to a 16ms frame once `to_bevy_mesh`
/// conversion, the GPU upload, and every other per-frame system are added on
/// top (and wasm typically runs slower still). Scaled down to leave real
/// headroom: ~25_000/50_000 * 13.9ms =~ 7ms for `build_ribbon_mesh` alone.
const FRAME_BUILD_BUDGET_EVENTS: usize = 25_000;
/// The active (playhead) layer's partial mesh is only rebuilt once it has
/// grown by this many events (or shrunk, or completed), so scrubbing inside
/// a single very dense layer doesn't rebuild its mesh every frame.
const ACTIVE_REBUILD_GRANULARITY: usize = 32;

struct LayerVisual {
    entity: Entity,
    mesh_handle: Handle<Mesh>,
    built_up_to: usize,
    /// Mirrors the entity's `Visibility` component so the loop only touches
    /// it on transitions instead of every frame.
    hidden: bool,
}

/// Tracks one mesh entity per print layer. Only the currently-active layer
/// (the one the playhead is inside) is rebuilt each frame, and that rebuild
/// is budgeted (`FRAME_BUILD_BUDGET_EVENTS`) along with any other layers a
/// jump needs to newly build, so a single frame never has to build the whole
/// toolpath. Layers the playhead has scrubbed back before are hidden (not
/// despawned) and shown again instantly if played forward, without a
/// rebuild.
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

    let mut budget = FRAME_BUILD_BUDGET_EVENTS;
    let mut built_something = false;

    for i in 0..visuals.layers.len() {
        let layer = visuals.layers[i];
        let (desired_end, want_visible) = match i.cmp(&current_layer) {
            std::cmp::Ordering::Less => (layer.end, true),
            std::cmp::Ordering::Equal => ((idx + 1).min(layer.end), true),
            std::cmp::Ordering::Greater => (layer.start, false),
        };
        let needs_geometry = desired_end > layer.start + 1;

        if !want_visible || !needs_geometry {
            // Hide rather than despawn: keeps the mesh (and its GPU
            // allocation) resident so scrubbing back over it later is a
            // free `Visibility` flip instead of a full rebuild.
            if let Some(visual) = visuals.visuals[i].as_mut() {
                if !visual.hidden {
                    commands.entity(visual.entity).insert(Visibility::Hidden);
                    visual.hidden = true;
                }
            }
            continue;
        }

        match visuals.visuals[i].as_mut() {
            Some(visual) if visual.built_up_to == desired_end => {
                // Already built to exactly what's needed: just unhide.
                if visual.hidden {
                    commands.entity(visual.entity).insert(Visibility::Inherited);
                    visual.hidden = false;
                }
            }
            Some(visual) => {
                if visual.hidden {
                    commands.entity(visual.entity).insert(Visibility::Inherited);
                    visual.hidden = false;
                }

                // The active layer's partial mesh is rebuilt only every
                // ACTIVE_REBUILD_GRANULARITY events, except when it must
                // shrink (scrubbed backward within it) or finish (reached
                // its end) -- both need to be exact immediately. Layers
                // ahead of the active one (i < current_layer) always need
                // their full, fixed `end`, so this gate doesn't apply.
                let is_shrink = desired_end < visual.built_up_to;
                let is_finalize = desired_end == layer.end;
                let grew_enough = desired_end >= visual.built_up_to + ACTIVE_REBUILD_GRANULARITY;
                if i == current_layer && !is_shrink && !is_finalize && !grew_enough {
                    continue;
                }

                let cost = desired_end - layer.start;
                // Always build at least one layer per frame (even if it
                // blows the budget alone) so a single oversized layer can't
                // stall progress forever.
                if cost > budget && built_something {
                    continue;
                }

                let mesh_data = build_ribbon_mesh(
                    &state.toolpath[layer.start..desired_end],
                    FILAMENT_WIDTH,
                    FILAMENT_HEIGHT,
                );
                // Same guard as below: never write an empty mesh into an
                // existing tracked handle. Leaving built_up_to unset means
                // this retries next frame (or next budget) with a larger
                // slice.
                if !mesh_data.is_empty() {
                    if let Some(mut slot) = meshes.get_mut(&visual.mesh_handle) {
                        *slot = to_bevy_mesh(mesh_data);
                    }
                    visual.built_up_to = desired_end;
                }
                // Charge the budget even on an empty result -- the CPU work
                // (the ribbon-mesh scan) happened either way.
                budget = budget.saturating_sub(cost);
                built_something = true;
            }
            None => {
                let cost = desired_end - layer.start;
                if cost > budget && built_something {
                    continue;
                }

                let mesh_data = build_ribbon_mesh(
                    &state.toolpath[layer.start..desired_end],
                    FILAMENT_WIDTH,
                    FILAMENT_HEIGHT,
                );
                budget = budget.saturating_sub(cost);
                built_something = true;

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
                        // Explicit rather than relying on Mesh3d's required-
                        // component default: this loop actively toggles
                        // Visibility later, so it must be a real, queryable
                        // component from the start.
                        Visibility::Inherited,
                    ))
                    .id();
                commands.entity(root).add_child(entity);
                visuals.visuals[i] = Some(LayerVisual {
                    entity,
                    mesh_handle: handle,
                    built_up_to: desired_end,
                    hidden: false,
                });
            }
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

    /// Like `layer_events`, but only events `1..=10` extrude; the rest are
    /// travel. Same event *count* (what the frame budget charges for), but a
    /// tiny actual mesh, so tests with large event counts stay fast in debug
    /// builds.
    fn sparse_layer_events(n: usize, t0: f64, x0: f32, z: f32) -> Vec<MotionEvent> {
        (0..n)
            .map(|i| ev(t0 + i as f64, x0 + i as f32, z, (1..=10).contains(&i)))
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

    /// A jump across a huge toolpath must not build every intervening
    /// layer's mesh in a single frame -- that's the multi-second stall that
    /// looked, to a user, like the timeline snapping back to the start.
    #[test]
    fn fast_forward_jump_builds_at_most_budget_per_frame() {
        const LAYERS: usize = 40;
        const PER_LAYER: usize = 4_000;

        let mut app = test_app();
        let mut events = Vec::with_capacity(LAYERS * PER_LAYER);
        for layer in 0..LAYERS {
            events.extend(sparse_layer_events(
                PER_LAYER,
                (layer * PER_LAYER) as f64,
                0.0,
                layer as f32 * 0.3,
            ));
        }
        let total_events = events.len();
        {
            let mut state = app.world_mut().resource_mut::<PrintState>();
            state.time = (total_events - 1) as f64;
            state.toolpath = events;
            state.generation = 1;
        }

        app.update();

        {
            let visuals = app.world().resource::<LayerVisuals>();
            assert_eq!(visuals.layer_count(), LAYERS);
            let mut built_count = 0usize;
            let mut built_cost = 0usize;
            for (i, visual) in visuals.visuals.iter().enumerate() {
                if let Some(visual) = visual {
                    built_count += 1;
                    built_cost += visual.built_up_to - visuals.layers[i].start;
                }
            }
            assert_eq!(
                built_count,
                FRAME_BUILD_BUDGET_EVENTS / PER_LAYER,
                "one frame should only build as many whole layers as the budget allows"
            );
            assert!(
                built_cost <= FRAME_BUILD_BUDGET_EVENTS,
                "built_cost {built_cost} exceeded the frame budget"
            );
        }

        let max_frames = total_events / FRAME_BUILD_BUDGET_EVENTS + 2;
        for _ in 0..max_frames {
            app.update();
        }

        let visuals = app.world().resource::<LayerVisuals>();
        for i in 0..LAYERS {
            let layer = visuals.layers[i];
            let visual = visuals.visuals[i]
                .as_ref()
                .unwrap_or_else(|| panic!("layer {i} should be fully built by now"));
            assert_eq!(
                visual.built_up_to, layer.end,
                "layer {i} should have progressively finished building"
            );
        }
    }

    /// A single layer bigger than the whole frame budget must still make
    /// progress in one frame rather than never being started.
    #[test]
    fn oversized_single_layer_still_makes_progress() {
        let mut app = test_app();

        let big_layer_len = FRAME_BUILD_BUDGET_EVENTS + 1_000;
        let mut events = sparse_layer_events(big_layer_len, 0.0, 0.0, 0.0);
        events.extend(sparse_layer_events(5, big_layer_len as f64, 0.0, 0.3));
        let total_events = events.len();
        {
            let mut state = app.world_mut().resource_mut::<PrintState>();
            state.time = (total_events - 1) as f64;
            state.toolpath = events;
            state.generation = 1;
        }

        app.update();

        let visuals = app.world().resource::<LayerVisuals>();
        assert_eq!(visuals.layer_count(), 2);
        let visual = visuals.visuals[0]
            .as_ref()
            .expect("the oversized layer must still be built in one frame");
        assert_eq!(visual.built_up_to, visuals.layers[0].end);
    }

    /// Scrubbing backward must hide (not despawn) layers ahead of the
    /// playhead, and scrubbing forward again must restore them without any
    /// mesh rebuild -- that's what makes wiggling the TIME slider instant
    /// instead of repeatedly despawning and rebuilding.
    #[test]
    fn scrub_back_hides_and_forward_restores_without_rebuild() {
        let mut app = test_app();

        let mut events = layer_events(10, 0.0, 0.0, 0.0);
        events.extend(layer_events(10, 10.0, 0.0, 0.3));
        events.extend(layer_events(10, 20.0, 0.0, 0.6));
        let total_events = events.len();
        {
            let mut state = app.world_mut().resource_mut::<PrintState>();
            state.toolpath = events;
            state.time = (total_events - 1) as f64;
            state.generation = 1;
        }
        // Small toolpath: a couple of frames is enough to build everything.
        app.update();
        app.update();

        let (entities_before, built_before, mesh_count_before) = {
            let visuals = app.world().resource::<LayerVisuals>();
            assert_eq!(visuals.layer_count(), 3);
            let entities: Vec<Entity> = visuals
                .visuals
                .iter()
                .map(|v| v.as_ref().expect("layer must be built").entity)
                .collect();
            let built: Vec<usize> = visuals
                .visuals
                .iter()
                .map(|v| v.as_ref().expect("layer must be built").built_up_to)
                .collect();
            let meshes = app.world().resource::<Assets<Mesh>>();
            (entities, built, meshes.iter().count())
        };

        // Scrub backward into layer 0.
        {
            let mut state = app.world_mut().resource_mut::<PrintState>();
            state.time = 5.0;
        }
        app.update();

        {
            let visuals = app.world().resource::<LayerVisuals>();
            for (i, expected_entity) in entities_before.iter().enumerate().skip(1) {
                let visual = visuals.visuals[i]
                    .as_ref()
                    .unwrap_or_else(|| panic!("layer {i} visual must survive a backward scrub"));
                assert_eq!(visual.entity, *expected_entity);
                assert!(visual.hidden, "layer {i} should be marked hidden");
            }
            let world = app.world();
            for (i, entity) in entities_before.iter().enumerate().skip(1) {
                assert_eq!(
                    world.get::<Visibility>(*entity),
                    Some(&Visibility::Hidden),
                    "layer {i}'s entity should carry Visibility::Hidden"
                );
            }
            let meshes = app.world().resource::<Assets<Mesh>>();
            assert_eq!(
                meshes.iter().count(),
                mesh_count_before,
                "no meshes should be added or removed on a backward scrub"
            );
        }

        // Scrub forward to the end again.
        {
            let mut state = app.world_mut().resource_mut::<PrintState>();
            state.time = (total_events - 1) as f64;
        }
        app.update();

        {
            let visuals = app.world().resource::<LayerVisuals>();
            for (i, expected_entity) in entities_before.iter().enumerate() {
                let visual = visuals.visuals[i]
                    .as_ref()
                    .unwrap_or_else(|| panic!("layer {i} visual must exist after scrubbing forward"));
                assert_eq!(
                    visual.entity, *expected_entity,
                    "layer {i} entity must be reused, not respawned"
                );
                assert!(!visual.hidden);
                assert_eq!(
                    visual.built_up_to, built_before[i],
                    "layer {i} must not be rebuilt just from an unhide"
                );
            }
            let world = app.world();
            for entity in &entities_before {
                assert_eq!(world.get::<Visibility>(*entity), Some(&Visibility::Inherited));
            }
            let meshes = app.world().resource::<Assets<Mesh>>();
            assert_eq!(
                meshes.iter().count(),
                mesh_count_before,
                "no meshes should be added or removed when restoring visibility"
            );
        }
    }

    /// The active layer's partial mesh should only rebuild once it has grown
    /// by `ACTIVE_REBUILD_GRANULARITY` events, except when it must shrink or
    /// finalize -- so scrubbing inside one dense layer doesn't rebuild every
    /// frame.
    #[test]
    fn active_layer_rebuild_is_granular() {
        let mut app = test_app();

        let events = layer_events(100, 0.0, 0.0, 0.0);
        {
            let mut state = app.world_mut().resource_mut::<PrintState>();
            state.toolpath = events;
            state.time = 10.0;
            state.generation = 1;
        }
        app.update();
        {
            let visuals = app.world().resource::<LayerVisuals>();
            assert_eq!(visuals.layer_count(), 1);
            let visual = visuals.visuals[0].as_ref().expect("layer must be built");
            assert_eq!(visual.built_up_to, 11);
        }

        // Grow, but by less than the granularity: no rebuild yet.
        {
            let mut state = app.world_mut().resource_mut::<PrintState>();
            state.time = 10.0 + (ACTIVE_REBUILD_GRANULARITY - 2) as f64;
        }
        app.update();
        {
            let visuals = app.world().resource::<LayerVisuals>();
            let visual = visuals.visuals[0].as_ref().expect("layer must be built");
            assert_eq!(
                visual.built_up_to, 11,
                "a small grow should not trigger a rebuild of the active layer"
            );
        }

        // Grow past the granularity: rebuild.
        {
            let mut state = app.world_mut().resource_mut::<PrintState>();
            state.time += 4.0;
        }
        app.update();
        {
            let visuals = app.world().resource::<LayerVisuals>();
            let visual = visuals.visuals[0].as_ref().expect("layer must be built");
            assert!(
                visual.built_up_to > 11,
                "growing past the granularity should trigger a rebuild"
            );
        }

        // Jump to the very end: finalize regardless of granularity.
        {
            let mut state = app.world_mut().resource_mut::<PrintState>();
            state.time = 99.0;
        }
        app.update();
        {
            let visuals = app.world().resource::<LayerVisuals>();
            let visual = visuals.visuals[0].as_ref().expect("layer must be built");
            assert_eq!(visual.built_up_to, 100);
        }
    }
}
