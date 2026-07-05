/**
 * RAMPS 1.4 pin assignments for the `RAMPS_14_EFB` board variant (1 hotend,
 * 1 fan, heated bed) -- Marlin's stock default motherboard, matching the
 * `assets/firmware/marlin_ramps14.hex` build. Source of truth:
 * `Marlin/src/pins/ramps/pins_RAMPS.h` in the Marlin source tree.
 */
export const RAMPS_PINS = {
  X_STEP: 54,
  X_DIR: 55,
  X_ENABLE: 38,
  X_MIN: 3,
  X_MAX: 2,

  Y_STEP: 60,
  Y_DIR: 61,
  Y_ENABLE: 56,
  Y_MIN: 14,
  Y_MAX: 15,

  Z_STEP: 46,
  Z_DIR: 48,
  Z_ENABLE: 62,
  Z_MIN: 18,
  Z_MAX: 19,

  E0_STEP: 26,
  E0_DIR: 28,
  E0_ENABLE: 24,

  HEATER_0: 10,
  HEATER_BED: 8,
  FAN0: 9,
} as const;

/** Analog channel numbers (already channel indices, not digital pin numbers). */
export const RAMPS_ANALOG = {
  TEMP_0: 13,
  TEMP_BED: 14,
} as const;

/** Default steps/mm for a stock RAMPS/Cartesian Marlin build (from Configuration.h). */
export const DEFAULT_STEPS_PER_MM = {
  X: 80,
  Y: 80,
  Z: 400,
  E: 500,
} as const;

/**
 * Default `INVERT_*_DIR` flags (Configuration.h). Only Y is inverted on a
 * stock RAMPS/Cartesian build. Verified empirically: with DIR pin HIGH
 * meaning "positive/away from home" on a non-inverted axis, a G28 on X
 * moved *away* from the endstop until this was accounted for.
 */
export const DEFAULT_INVERT_DIR = {
  X: false,
  Y: true,
  Z: false,
  E: false,
} as const;
