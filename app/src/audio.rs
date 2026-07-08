use bevy::prelude::*;

/// Sound-effect requests, written by the UI and alert systems. Always
/// registered so writers compile unconditionally; only consumed (and heard)
/// when the `audio` cargo feature is enabled.
#[derive(Message)]
pub enum SfxEvent {
    Click,
    Beep,
    DataConfirm,
    CodecCall,
}

/// Global sound on/off toggle, written by the UI's MUTE checkbox. Always
/// registered (like [`SfxEvent`]) so the checkbox compiles and works
/// regardless of the `audio` feature; only consulted by the systems below
/// when there's actually sound to suppress.
#[derive(Resource, Default)]
pub struct AudioSettings {
    pub muted: bool,
}

#[cfg(feature = "audio")]
mod imp {
    use bevy::audio::{AudioPlayer, AudioSink, AudioSource, PlaybackSettings, Volume};
    use bevy::prelude::*;

    use super::{AudioSettings, SfxEvent};
    use crate::kinematics::HeadVelocity;

    /// Head speed (mm/s) mapped to full hum pitch/volume.
    const HUM_FULL_SPEED: f32 = 150.0;
    /// Warble rate (Hz) layered onto the hum pitch while agitated (sharp
    /// turns / zig-zag infill), giving direction changes an audibly busier
    /// character instead of the hum staying identical to a smooth glide.
    const AGITATION_WARBLE_HZ: f32 = 38.0;

    #[derive(Resource)]
    pub struct AudioHandles {
        click: Handle<AudioSource>,
        beep: Handle<AudioSource>,
        data_confirm: Handle<AudioSource>,
        codec_call: Handle<AudioSource>,
    }

    /// The looping stepper-motor hum; starts paused (which also satisfies
    /// browser autoplay policy: it first unpauses after a user gesture has
    /// loaded a toolpath and the head is moving).
    #[derive(Component)]
    pub struct StepperHum;

    pub fn setup(mut commands: Commands, asset_server: Res<AssetServer>) {
        commands.insert_resource(AudioHandles {
            click: asset_server.load("audio/ui_click.wav"),
            beep: asset_server.load("audio/codec_beep.wav"),
            data_confirm: asset_server.load("audio/data_confirm.wav"),
            codec_call: asset_server.load("audio/codec_call.wav"),
        });
        commands.spawn((
            StepperHum,
            AudioPlayer::new(asset_server.load("audio/stepper_hum.wav")),
            PlaybackSettings::LOOP.paused(),
        ));
    }

    pub fn play_sfx(
        mut commands: Commands,
        mut events: MessageReader<SfxEvent>,
        handles: Res<AudioHandles>,
        settings: Res<AudioSettings>,
    ) {
        for event in events.read() {
            // Still drain the reader while muted (skip via `continue`, not an
            // early `return`) so events don't pile up and all replay at once
            // the moment sound is re-enabled.
            if settings.muted {
                continue;
            }
            let handle = match event {
                SfxEvent::Click => &handles.click,
                SfxEvent::Beep => &handles.beep,
                SfxEvent::DataConfirm => &handles.data_confirm,
                SfxEvent::CodecCall => &handles.codec_call,
            };
            commands.spawn((AudioPlayer::new(handle.clone()), PlaybackSettings::DESPAWN));
        }
    }

    /// Pitch and volume of the stepper hum follow the executed head speed,
    /// with an added warble/buzz on top scaled by `HeadVelocity.agitation`
    /// -- so sharp turns and zig-zag infill sound audibly busier than a
    /// long straight glide at the same speed.
    pub fn stepper_audio(
        time: Res<Time>,
        velocity: Res<HeadVelocity>,
        settings: Res<AudioSettings>,
        mut hum: Query<&mut AudioSink, With<StepperHum>>,
    ) {
        let Ok(mut sink) = hum.single_mut() else {
            return;
        };
        if settings.muted {
            if !sink.is_paused() {
                sink.pause();
            }
            return;
        }
        let speed = velocity.mm_per_s;
        if speed <= 0.05 {
            if !sink.is_paused() {
                sink.pause();
            }
            return;
        }
        if sink.is_paused() {
            sink.play();
        }
        let x = (speed / HUM_FULL_SPEED).clamp(0.0, 1.0);
        let agitation = velocity.agitation;
        let warble = (time.elapsed_secs() * AGITATION_WARBLE_HZ).sin();
        let pitch = 0.85 + 0.5 * x + agitation * (0.12 + 0.10 * warble);
        let volume = 0.12 + 0.45 * x + 0.1 * agitation;
        sink.set_speed(pitch.max(0.1));
        sink.set_volume(Volume::Linear(volume.clamp(0.0, 1.0)));
    }
}

#[cfg(feature = "audio")]
pub use imp::*;
