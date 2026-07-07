// Bevy systems idiomatically take many Query/Res/ResMut parameters, and
// ParamSet/async Task types are inherently "complex" by these lints'
// heuristics; Bevy's own crates disable both project-wide for this reason.
#![allow(clippy::too_many_arguments, clippy::type_complexity)]

use bevy::prelude::*;
use bevy_egui::{EguiPlugin, EguiStartupSet};

mod audio;
mod bgcode;
mod camera;
mod coords;
mod file_picker;
mod kinematics;
mod layers;
mod loader;
mod panels;
mod playback;
mod printer_model;
mod printer_rig;
mod psx;
mod theme;
mod ui;

#[cfg(target_arch = "wasm32")]
mod firmware;
#[cfg(target_arch = "wasm32")]
mod wasm_drop;

fn main() {
    #[cfg(target_arch = "wasm32")]
    wasm_drop::install_drop_listener();

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "3D Printer Simulator".to_string(),
            fit_canvas_to_parent: true,
            ..default()
        }),
        ..default()
    }))
    .add_plugins(EguiPlugin::default())
    .insert_resource(ClearColor(psx::FOG_COLOR))
    .init_resource::<playback::PrintState>()
    .init_resource::<kinematics::HeadVelocity>()
    .init_resource::<printer_rig::PendingRigParts>()
    .init_resource::<layers::LayerVisuals>()
    .init_resource::<ui::UiState>()
    .init_resource::<ui::AlertState>()
    .init_resource::<ui::PointerOverUi>()
    .init_resource::<file_picker::PendingGcodePick>()
    .add_message::<audio::SfxEvent>()
    .add_systems(
        PreStartup,
        (
            psx::disable_auto_egui_context,
            psx::create_psx_canvas,
            camera::setup_camera,
            psx::setup_outer_camera,
        )
            .chain()
            .before(EguiStartupSet::InitContexts),
    )
    .add_systems(
        Startup,
        (printer_model::setup_scene, printer_rig::spawn_printer_scene),
    );

    #[cfg(not(target_arch = "wasm32"))]
    app.add_systems(Startup, loader::autoload_from_env);

    #[cfg(feature = "audio")]
    app.add_systems(Startup, audio::setup)
        .add_systems(Update, (audio::play_sfx, audio::stepper_audio));

    app.add_systems(
        Update,
        (
            loader::handle_file_drop,
            file_picker::poll_file_pick,
            playback::advance_time,
            layers::update_layer_meshes,
            kinematics::drive_kinematics,
            printer_rig::discover_rig_parts,
            camera::orbit_camera,
            psx::fit_canvas,
            ui::update_alerts,
            ui::keyboard_toggles,
        ),
    )
    .add_systems(bevy_egui::EguiPrimaryContextPass, ui::playback_ui);

    #[cfg(target_arch = "wasm32")]
    {
        app.init_resource::<firmware::FirmwareState>();
        app.add_systems(
            Update,
            (wasm_drop::poll_dropped_file, firmware::drive_firmware),
        );
    }

    app.run();
}
