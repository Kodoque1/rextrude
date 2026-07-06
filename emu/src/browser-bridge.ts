/**
 * The JS-side half of the firmware emulation bridge (see phase B plan).
 * Bundled (via esbuild) into a single self-contained ES module that the
 * Bevy/wasm-bindgen app imports directly -- Rust calls `tickEmulator()`
 * once per frame and reads the results back through a few plain-typed
 * getters, rather than JS needing to call back into Rust's exports (which
 * would require importing wasm-bindgen's hashed output filename) or Rust
 * needing to deserialize a complex JsValue.
 */
import { RampsBoard } from './board.js';
import { parseIntelHex } from './ihex.js';
import { createMega2560, MEGA2560_FREQ_HZ } from './mega2560.js';
import { stepCpu } from './run-loop.js';
import { ThermalSim } from './thermal.js';
import { UartHost } from './uart-host.js';

const FLASH_SIZE = 256 * 1024;
const THERMAL_STEP_SECONDS = 0.1;
const THERMAL_STEP_CYCLES = Math.floor(MEGA2560_FREQ_HZ * THERMAL_STEP_SECONDS);
const ADC_HOTEND_CHANNEL = 13;
const ADC_BED_CHANNEL = 14;

interface EmulatorState {
  mega: ReturnType<typeof createMega2560>;
  board: RampsBoard;
  thermal: ThermalSim;
  host: UartHost;
  nextThermalTick: number;
  uartLines: string[];
}

let state: EmulatorState | null = null;

export function createEmulator(hexText: string): void {
  const flashBytes = parseIntelHex(hexText, FLASH_SIZE);
  const progMem = new Uint16Array(
    flashBytes.buffer,
    flashBytes.byteOffset,
    flashBytes.byteLength / 2,
  );

  const mega = createMega2560(progMem);
  mega.adc.channelValues.fill(4.78); // plausible room-temp voltage until the thermal model overwrites it

  const board = new RampsBoard(mega);
  const thermal = new ThermalSim();
  const host = new UartHost(mega.usart0);

  const uartLines: string[] = [];
  host.onLine = (line) => {
    uartLines.push(line);
    if (uartLines.length > 500) uartLines.shift();
  };

  state = { mega, board, thermal, host, nextThermalTick: THERMAL_STEP_CYCLES, uartLines };
}

export function isEmulatorLoaded(): boolean {
  return state !== null;
}

export function sendGcode(text: string): void {
  state?.host.enqueueGcode(text);
}

export function cyclesPerSecond(): number {
  return MEGA2560_FREQ_HZ;
}

export function currentCycles(): number {
  return state?.mega.cpu.cycles ?? 0;
}

export function hotendCelsius(): number {
  return state?.thermal.hotend.celsius ?? 0;
}

export function bedCelsius(): number {
  return state?.thermal.bed.celsius ?? 0;
}

/** Joined by '\n'; wasm-bindgen maps Vec<String> awkwardly, a single string is simpler. */
export function drainUartText(): string {
  if (!state) return '';
  const lines = state.uartLines.splice(0, state.uartLines.length);
  return lines.join('');
}

/** Advances the emulator by up to `cycleBudget` CPU cycles. Call the getters after this. */
export function tickEmulator(cycleBudget: number): void {
  if (!state) return;
  const { mega, board, thermal } = state;
  const { cpu, adc } = mega;

  const deadline = cpu.cycles + cycleBudget;
  while (cpu.cycles < deadline) {
    stepCpu(cpu);
    if (cpu.cycles >= state.nextThermalTick) {
      thermal.step(THERMAL_STEP_SECONDS, board.drainHeaterDuty(), adc, {
        hotend: ADC_HOTEND_CHANNEL,
        bed: ADC_BED_CHANNEL,
      });
      state.nextThermalTick += THERMAL_STEP_CYCLES;
    }
  }
}

/** Flattened [cycle, x, y, z, e] tuples, one per step event since the last drain. */
export function drainStepEvents(): Float64Array {
  if (!state) return new Float64Array(0);
  const events = state.board.drainStepEvents();
  const flat = new Float64Array(events.length * 5);
  for (let i = 0; i < events.length; i++) {
    const ev = events[i];
    flat[i * 5] = ev.cycle;
    flat[i * 5 + 1] = ev.x;
    flat[i * 5 + 2] = ev.y;
    flat[i * 5 + 3] = ev.z;
    flat[i * 5 + 4] = ev.e;
  }
  return flat;
}
