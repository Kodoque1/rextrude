use bevy::prelude::Vec3;

/// Gcode is Z-up (X/Y across the bed, Z is print height); Bevy is Y-up.
/// Maps gcode millimeters directly to Bevy world units (1 unit = 1 mm).
pub fn gcode_to_bevy(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, z, y)
}
