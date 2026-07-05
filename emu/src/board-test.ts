/**
 * Checkpoint: boot Marlin, wait for it to settle, send G28 (home all axes),
 * and confirm the RampsBoard's step/endstop wiring actually lets homing
 * complete (i.e. Marlin sees the endstops trigger and doesn't hang forever).
 */
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import { RampsBoard } from './board.js';
import { parseIntelHex } from './ihex.js';
import { createMega2560, MEGA2560_FREQ_HZ } from './mega2560.js';
import { stepCpu } from './run-loop.js';

const FLASH_SIZE = 256 * 1024;
const hexPath = fileURLToPath(new URL('../../assets/firmware/marlin_ramps14.hex', import.meta.url));
const hexText = readFileSync(hexPath, 'utf-8');
const flashBytes = parseIntelHex(hexText, FLASH_SIZE);
const progMem = new Uint16Array(flashBytes.buffer, flashBytes.byteOffset, flashBytes.byteLength / 2);

const mega = createMega2560(progMem);
const { cpu, usart0, adc } = mega;
adc.channelValues.fill(4.78);

const board = new RampsBoard(mega);
// Start axes away from the origin so homing actually has to move.
board.position.X = 50;
board.position.Y = 50;
board.position.Z = 50;

let serialOut = '';
let sinceLastOk = '';
usart0.onByteTransmit = (value) => {
  const ch = String.fromCharCode(value);
  serialOut += ch;
  sinceLastOk += ch;
  process.stdout.write(ch);
};

function runFor(seconds: number) {
  const deadline = cpu.cycles + MEGA2560_FREQ_HZ * seconds;
  while (cpu.cycles < deadline) {
    stepCpu(cpu);
  }
}

console.log('--- booting ---');
runFor(3);
if (!serialOut.includes('Marlin')) {
  console.error('FAIL: no boot banner');
  process.exit(1);
}

console.log('\n--- sending G28 ---');
sinceLastOk = '';
const command = 'G28\n';
let i = 0;
usart0.onRxComplete = () => {
  i++;
  if (i < command.length) usart0.writeByte(command.charCodeAt(i));
};
usart0.writeByte(command.charCodeAt(0));

const homeDeadline = cpu.cycles + MEGA2560_FREQ_HZ * 40;
while (cpu.cycles < homeDeadline && !sinceLastOk.includes('\nok')) {
  stepCpu(cpu);
}

console.log(`\nsteps recorded: ${board.stepEvents.length}`);
console.log('position after G28:', board.position);

if (!sinceLastOk.includes('\nok')) {
  console.error('FAIL: G28 never completed (no "ok" within 10s virtual time) -- likely stuck waiting on an endstop');
  process.exit(1);
}

const settled = ['X', 'Y', 'Z'].every((axis) => Math.abs(board.position[axis as 'X' | 'Y' | 'Z']) < 1);
if (!settled) {
  console.error('FAIL: axes did not end up near 0 after homing', board.position);
  process.exit(1);
}

console.log('OK: G28 completed and all axes homed near 0');
console.log('--- board harness checkpoint PASSED ---');
