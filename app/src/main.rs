use bevy::prelude::*;
use bevy_egui::{EguiPlugin, EguiStartupSet};

mod camera;
mod coords;
mod kinematics;
mod layers;
mod loader;
mod playback;
mod printer_model;
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
            ..default()
        }),
        ..default()
    }))
    .add_plugins(EguiPlugin::default())
    .init_resource::<playback::PrintState>()
    .init_resource::<kinematics::HeadVelocity>()
    .init_resource::<layers::LayerVisuals>()
    .init_resource::<ui::UiState>()
    .add_systems(
        PreStartup,
        camera::setup_camera.before(EguiStartupSet::InitContexts),
    )
    .add_systems(Startup, printer_model::setup_scene);

    #[cfg(not(target_arch = "wasm32"))]
    app.add_systems(Startup, loader::autoload_from_env);

    app
    .add_systems(
        Update,
        (
            loader::handle_file_drop,
            playback::advance_time,
            layers::update_layer_meshes,
            kinematics::drive_kinematics,
            camera::orbit_camera,
        ),
    )
    .add_systems(bevy_egui::EguiPrimaryContextPass, ui::playback_ui);

    #[cfg(target_arch = "wasm32")]
    {
        app.init_resource::<firmware::FirmwareState>();
        app.add_systems(Update, (wasm_drop::poll_dropped_file, firmware::drive_firmware));
    }

    app.run();
}
