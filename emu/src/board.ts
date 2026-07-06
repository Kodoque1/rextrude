import type { AVRIOPort } from 'avr8js';

import type { Mega2560 } from './mega2560.js';
import { MEGA2560_PIN_TABLE, type PortLetter } from './pin-table.js';
import { DEFAULT_INVERT_DIR, DEFAULT_STEPS_PER_MM, RAMPS_PINS } from './ramps-pins.js';

export interface StepEvent {
  /** CPU cycle count at which this step was taken. */
  cycle: number;
  x: number;
  y: number;
  z: number;
  e: number;
}

type Axis = 'X' | 'Y' | 'Z' | 'E';

function bit(value: number, index: number): boolean {
  return (value & (1 << index)) !== 0;
}

/**
 * Marlin drives heaters via software PWM: the temperature ISR toggles the
 * raw output pin at a duty cycle it computes internally, rather than using
 * a hardware timer. We only see the pin toggling, so to recover a duty
 * fraction for the thermal model we have to integrate on-time ourselves
 * between `drainDuty()` calls, rather than sampling instantaneous state.
 */
class DutyTracker {
  private on = false;
  private onCycles = 0;
  private lastChangeCycle: number;
  private windowStartCycle: number;

  constructor(private readonly cpu: { cycles: number }) {
    this.lastChangeCycle = cpu.cycles;
    this.windowStartCycle = cpu.cycles;
  }

  setState(on: boolean) {
    const now = this.cpu.cycles;
    if (this.on) this.onCycles += now - this.lastChangeCycle;
    this.on = on;
    this.lastChangeCycle = now;
  }

  /** Fraction of time spent ON since the last call, in [0, 1]. */
  drainDuty(): number {
    const now = this.cpu.cycles;
    if (this.on) this.onCycles += now - this.lastChangeCycle;
    this.lastChangeCycle = now;
    const windowCycles = now - this.windowStartCycle;
    this.windowStartCycle = now;
    const duty = windowCycles > 0 ? this.onCycles / windowCycles : 0;
    this.onCycles = 0;
    return duty;
  }
}

/**
 * Wires up a RAMPS 1.4 board on top of a running ATmega2560: decodes
 * step/dir pulses into a position-over-time log (the RAMPS/firmware
 * equivalent of `gcode-sim`'s `MotionEvent` stream), and drives endstop pins
 * automatically once an axis reaches its origin (mirroring the simplified
 * "endstop triggers at position 0" model used by the gcode-sim backend, so
 * both backends behave the same way for G28 homing).
 */
export class RampsBoard {
  readonly stepEvents: StepEvent[] = [];
  readonly position: Record<Axis, number> = { X: 0, Y: 0, Z: 0, E: 0 };

  hotendOn = false;
  bedOn = false;
  fanOn = false;

  private readonly hotendDuty: DutyTracker;
  private readonly bedDuty: DutyTracker;
  private readonly fanDuty: DutyTracker;

  private readonly mega: Mega2560;
  private readonly dir: Record<Axis, 1 | -1> = { X: 1, Y: 1, Z: 1, E: 1 };
  /** Endstops fire once position crosses back to <= this margin, matching a real switch's engagement point. */
  private readonly endstopMargin = 0.05;

  constructor(mega: Mega2560) {
    this.mega = mega;
    this.hotendDuty = new DutyTracker(mega.cpu);
    this.bedDuty = new DutyTracker(mega.cpu);
    this.fanDuty = new DutyTracker(mega.cpu);

    this.attachAxisPort('F', {
      step: [RAMPS_PINS.X_STEP, 'X'],
      dir: [RAMPS_PINS.X_DIR, 'X'],
    });
    this.attachAxisPort('F', {
      step: [RAMPS_PINS.Y_STEP, 'Y'],
      dir: [RAMPS_PINS.Y_DIR, 'Y'],
    });
    this.attachAxisPort('L', {
      step: [RAMPS_PINS.Z_STEP, 'Z'],
      dir: [RAMPS_PINS.Z_DIR, 'Z'],
    });
    this.attachAxisPort('A', {
      step: [RAMPS_PINS.E0_STEP, 'E'],
      dir: [RAMPS_PINS.E0_DIR, 'E'],
    });

    this.attachHeaterPin(RAMPS_PINS.HEATER_0, (on) => {
      this.hotendOn = on;
      this.hotendDuty.setState(on);
    });
    this.attachHeaterPin(RAMPS_PINS.HEATER_BED, (on) => {
      this.bedOn = on;
      this.bedDuty.setState(on);
    });
    this.attachHeaterPin(RAMPS_PINS.FAN0, (on) => {
      this.fanOn = on;
      this.fanDuty.setState(on);
    });

    // This firmware build compiles *_MIN_ENDSTOP_INVERTING=false, which (as
    // verified empirically against a G28 run) means Marlin reads a HIGH
    // pin as "triggered" -- so idle (not triggered) is LOW here.
    this.setEndstopPin(RAMPS_PINS.X_MIN, false);
    this.setEndstopPin(RAMPS_PINS.Y_MIN, false);
    this.setEndstopPin(RAMPS_PINS.Z_MIN, false);
  }

  private port(letter: PortLetter): AVRIOPort {
    return this.mega.ports[letter];
  }

  private attachAxisPort(
    portLetter: PortLetter,
    signals: { step: [number, Axis]; dir: [number, Axis] },
  ) {
    const stepLoc = MEGA2560_PIN_TABLE[signals.step[0]];
    const dirLoc = MEGA2560_PIN_TABLE[signals.dir[0]];
    const axis = signals.step[1];
    const port = this.port(portLetter);

    port.addListener((value, oldValue) => {
      if (bit(value, dirLoc.bit) !== bit(oldValue, dirLoc.bit)) {
        // sign = +1 when (raw DIR pin HIGH) XOR (axis is direction-inverted).
        this.dir[axis] = bit(value, dirLoc.bit) !== DEFAULT_INVERT_DIR[axis] ? 1 : -1;
      }
      const wasHigh = bit(oldValue, stepLoc.bit);
      const isHigh = bit(value, stepLoc.bit);
      if (!wasHigh && isHigh) {
        this.recordStep(axis);
      }
    });
  }

  private recordStep(axis: Axis) {
    const stepsPerMm = DEFAULT_STEPS_PER_MM[axis];
    this.position[axis] += this.dir[axis] / stepsPerMm;
    this.stepEvents.push({
      cycle: this.mega.cpu.cycles,
      x: this.position.X,
      y: this.position.Y,
      z: this.position.Z,
      e: this.position.E,
    });

    if (axis === 'X' || axis === 'Y' || axis === 'Z') {
      const triggered = this.position[axis] <= this.endstopMargin;
      const pin =
        axis === 'X' ? RAMPS_PINS.X_MIN : axis === 'Y' ? RAMPS_PINS.Y_MIN : RAMPS_PINS.Z_MIN;
      this.setEndstopPin(pin, triggered);
    }
  }

  private setEndstopPin(arduinoPin: number, triggered: boolean) {
    const loc = MEGA2560_PIN_TABLE[arduinoPin];
    this.port(loc.port).setPin(loc.bit, triggered);
  }

  private attachHeaterPin(arduinoPin: number, onChange: (on: boolean) => void) {
    const loc = MEGA2560_PIN_TABLE[arduinoPin];
    this.port(loc.port).addListener((value) => {
      onChange(bit(value, loc.bit));
    });
  }

  /** Drains and returns all step events recorded since the last call. */
  drainStepEvents(): StepEvent[] {
    return this.stepEvents.splice(0, this.stepEvents.length);
  }

  /** Fraction of time [0,1] each heater/fan was on since the last call. */
  drainHeaterDuty(): { hotend: number; bed: number; fan: number } {
    return {
      hotend: this.hotendDuty.drainDuty(),
      bed: this.bedDuty.drainDuty(),
      fan: this.fanDuty.drainDuty(),
    };
  }
}
