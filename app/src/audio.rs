use bevy::prelude::*;

/// Sound-effect requests, written by the UI and alert systems. Always
/// registered so writers compile unconditionally; only consumed (and heard)
/// when the `audio` cargo feature is enabled.
#[derive(Message)]
pub enum SfxEvent {
    Click,
    Beep,
    Alert,
    CodecCall,
}

#[cfg(feature = "audio")]
mod imp {
    use bevy::audio::{AudioPlayer, AudioSink, AudioSource, PlaybackSettings, Volume};
    use bevy::prelude::*;

    use super::SfxEvent;
    use crate::kinematics::HeadVelocity;

    /// Head speed (mm/s) mapped to full hum pitch/volume.
    const HUM_FULL_SPEED: f32 = 150.0;

    #[derive(Resource)]
    pub struct AudioHandles {
        click: Handle<AudioSource>,
        beep: Handle<AudioSource>,
        alert: Handle<AudioSource>,
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
            alert: asset_server.load("audio/alert.wav"),
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
    ) {
        for event in events.read() {
            let handle = match event {
                SfxEvent::Click => &handles.click,
                SfxEvent::Beep => &handles.beep,
                SfxEvent::Alert => &handles.alert,
                SfxEvent::CodecCall => &handles.codec_call,
            };
            commands.spawn((AudioPlayer::new(handle.clone()), PlaybackSettings::DESPAWN));
        }
    }

    /// Pitch and volume of the stepper hum follow the executed head speed.
    pub fn stepper_audio(
        velocity: Res<HeadVelocity>,
        mut hum: Query<&mut AudioSink, With<StepperHum>>,
    ) {
        let Ok(mut sink) = hum.single_mut() else {
            return;
        };
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
        sink.set_speed(0.85 + 0.5 * x);
        sink.set_volume(Volume::Linear(0.12 + 0.45 * x));
    }
}

#[cfg(feature = "audio")]
pub use imp::*;
