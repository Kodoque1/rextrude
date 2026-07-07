use bevy::camera::visibility::RenderLayers;
use bevy::camera::RenderTarget;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::prelude::*;
use bevy::render::render_resource::{
    Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
};
use bevy::window::PrimaryWindow;
use bevy_egui::{EguiGlobalSettings, PrimaryEguiContext};

/// Internal render resolution: chunky enough to read as PSX-era, wide enough
/// to keep the mechanism legible.
pub const RES_WIDTH: u32 = 640;
pub const RES_HEIGHT: u32 = 360;

/// Everything on this layer is drawn by the outer (window) camera at native
/// resolution: the upscaled canvas sprite and, indirectly, egui.
const HIGH_RES_LAYER: RenderLayers = RenderLayers::layer(1);

/// The PSX look leans on a dark green void swallowing the scene edges.
pub const FOG_COLOR: Color = Color::srgb(0.030, 0.055, 0.038);

/// The sprite the low-res 3D frame is blitted onto.
#[derive(Component)]
pub struct PsxCanvas;

/// Handle to the low-res canvas image, created before the cameras spawn so
/// the 3D camera can target it from birth.
#[derive(Resource)]
pub struct PsxCanvasImage(pub Handle<Image>);

/// The window-facing camera (also hosts the primary egui context).
#[derive(Component)]
pub struct OuterCamera;

/// If bevy_egui auto-attached its context to the first camera it saw, it
/// would pick the render-to-texture 3D camera and draw the GUI at 640x360;
/// disable that before any camera exists and claim the context manually on
/// the outer camera below.
pub fn disable_auto_egui_context(mut settings: ResMut<EguiGlobalSettings>) {
    settings.auto_create_primary_context = false;
}

/// Creates the low-res canvas image (before the cameras spawn).
pub fn create_psx_canvas(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let canvas_size = Extent3d {
        width: RES_WIDTH,
        height: RES_HEIGHT,
        ..default()
    };
    let mut canvas = Image {
        texture_descriptor: TextureDescriptor {
            label: Some("psx canvas"),
            size: canvas_size,
            dimension: TextureDimension::D2,
            format: TextureFormat::Bgra8UnormSrgb,
            mip_level_count: 1,
            sample_count: 1,
            usage: TextureUsages::TEXTURE_BINDING
                | TextureUsages::COPY_DST
                | TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        },
        ..default()
    };
    canvas.resize(canvas_size);
    // Nearest-neighbor upscale: crisp fat pixels instead of bilinear mush.
    canvas.sampler = bevy::image::ImageSampler::nearest();
    commands.insert_resource(PsxCanvasImage(images.add(canvas)));
}

/// Components that point the 3D orbit camera at the canvas and give the
/// scene its PSX-era mood (no tonemap, hard pixels, green fog).
pub fn psx_camera_3d_bundle(canvas: &PsxCanvasImage) -> impl Bundle {
    (
        Camera {
            order: -1,
            clear_color: ClearColorConfig::Custom(FOG_COLOR),
            ..default()
        },
        RenderTarget::Image(canvas.0.clone().into()),
        Msaa::Off,
        Tonemapping::None,
        DistanceFog {
            color: FOG_COLOR,
            falloff: FogFalloff::Linear {
                start: 600.0,
                end: 1500.0,
            },
            ..default()
        },
    )
}

/// Spawns the canvas sprite and the outer window camera that upscales it
/// (and hosts the primary egui context).
pub fn setup_outer_camera(mut commands: Commands, canvas: Res<PsxCanvasImage>) {
    commands.spawn((
        PsxCanvas,
        Sprite::from_image(canvas.0.clone()),
        Transform::default(),
        HIGH_RES_LAYER,
    ));

    commands.spawn((
        OuterCamera,
        Camera2d,
        Msaa::Off,
        PrimaryEguiContext,
        HIGH_RES_LAYER,
    ));
}

/// Scales the canvas to fit the window (aspect-preserving; the shorter
/// axis is letterboxed with fog color rather than cropping the scene).
/// Recomputed every frame from the primary window's live size rather than
/// only on `WindowResized` events: on web the canvas can already be at its
/// full parent size before any resize event is delivered, which otherwise
/// leaves the projection stuck at its default scale (rendering the fixed
/// 640x360 texture at literal 1:1 size in the middle of the window).
pub fn fit_canvas(
    mut projection: Single<&mut Projection, With<OuterCamera>>,
    window: Single<&Window, With<PrimaryWindow>>,
) {
    let Projection::Orthographic(projection) = &mut **projection else {
        return;
    };
    let (width, height) = (window.width(), window.height());
    if width <= 0.0 || height <= 0.0 {
        return;
    }
    let scale = 1.0 / (width / RES_WIDTH as f32).min(height / RES_HEIGHT as f32);
    if projection.scale != scale {
        projection.scale = scale;
    }
}
