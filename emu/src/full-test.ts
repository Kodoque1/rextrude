/**
 * Checkpoint: boot Marlin, stream a real gcode file (with M104/M109/M140/M190
 * heating waits) over the UartHost, with the ThermalSim driving the ADC
 * channels -- confirming the thermal model actually lets the heating waits
 * unblock, and the whole file streams through to completion.
 */
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import { RampsBoard } from './board.js';
import { parseIntelHex } from './ihex.js';
import { createMega2560, MEGA2560_FREQ_HZ } from './mega2560.js';
import { stepCpu } from './run-loop.js';
import { ThermalSim } from './thermal.js';
import { UartHost } from './uart-host.js';

const FLASH_SIZE = 256 * 1024;
const hexPath = fileURLToPath(new URL('../../assets/firmware/marlin_ramps14.hex', import.meta.url));
const hexText = readFileSync(hexPath, 'utf-8');
const flashBytes = parseIntelHex(hexText, FLASH_SIZE);
const progMem = new Uint16Array(flashBytes.buffer, flashBytes.byteOffset, flashBytes.byteLength / 2);

const mega = createMega2560(progMem);
const { cpu, usart0, adc } = mega;
adc.channelValues.fill(4.78);

const board = new RampsBoard(mega);
const thermal = new ThermalSim();
const host = new UartHost(usart0);

let sawError = false;
host.onLine = (line) => {
  process.stdout.write(line);
  if (/Error:|MAXTEMP|MINTEMP/.test(line)) sawError = true;
};

const THERMAL_STEP_CYCLES = Math.floor(MEGA2560_FREQ_HZ * 0.1); // 100ms
let nextThermalTick = THERMAL_STEP_CYCLES;

function runUntil(predicate: () => boolean, maxSeconds: number): boolean {
  const deadline = cpu.cycles + MEGA2560_FREQ_HZ * maxSeconds;
  while (cpu.cycles < deadline) {
    stepCpu(cpu);
    if (cpu.cycles >= nextThermalTick) {
      thermal.step(0.1, board.drainHeaterDuty(), adc, { hotend: 13, bed: 14 });
      nextThermalTick += THERMAL_STEP_CYCLES;
    }
    if (predicate()) return true;
  }
  return false;
}

console.log('--- booting ---');
runUntil(() => false, 3);

console.log('\n--- streaming calibration_cube.gcode ---');
const gcodePath = fileURLToPath(new URL('../../assets/gcode/calibration_cube.gcode', import.meta.url));
host.enqueueGcode(readFileSync(gcodePath, 'utf-8'));

const finished = runUntil(() => host.linesRemaining === 0, 150);

console.log(`\nhotend: ${thermal.hotend.celsius.toFixed(1)}C, bed: ${thermal.bed.celsius.toFixed(1)}C`);
console.log(`steps recorded: ${board.stepEvents.length}, final position:`, board.position);

if (sawError) {
  console.error('FAIL: saw an Error/MAXTEMP/MINTEMP line during the print');
  process.exit(1);
}
if (!finished) {
  console.error('FAIL: gcode file did not finish streaming within the time budget');
  process.exit(1);
}

console.log('OK: whole file streamed and acknowledged with no thermal errors');
console.log('--- thermal + uart-host checkpoint PASSED ---');
