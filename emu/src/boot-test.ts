/**
 * Feasibility checkpoint (phase B): load the compiled Marlin RAMPS 1.4 .hex
 * into our ATmega2560 harness and confirm it boots -- i.e. we see Marlin's
 * startup banner over USART0, and it answers M115.
 */
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import { parseIntelHex } from './ihex.js';
import { createMega2560, MEGA2560_FREQ_HZ } from './mega2560.js';
import { stepCpu } from './run-loop.js';

const FLASH_SIZE = 256 * 1024;
const hexPath = fileURLToPath(new URL('../../assets/firmware/marlin_ramps14.hex', import.meta.url));
const hexText = readFileSync(hexPath, 'utf-8');
const flashBytes = parseIntelHex(hexText, FLASH_SIZE);
const progMem = new Uint16Array(
  flashBytes.buffer,
  flashBytes.byteOffset,
  flashBytes.byteLength / 2,
);

const mega = createMega2560(progMem);
const { cpu, usart0, adc } = mega;

// No thermal model yet (that's task 13) -- park every ADC channel at a
// plausible "100k NTC at room temperature, 4.7k pullup to 5V" voltage so
// Marlin's MAXTEMP/MINTEMP safety checks don't immediately trip while we're
// just checking that the firmware boots and talks over serial.
adc.channelValues.fill(4.78);

let serialOut = '';
usart0.onByteTransmit = (value) => {
  const ch = String.fromCharCode(value);
  serialOut += ch;
  process.stdout.write(ch);
};

const CYCLES_PER_SECOND = MEGA2560_FREQ_HZ;
const SIM_SECONDS = 3;
const totalCycles = CYCLES_PER_SECOND * SIM_SECONDS;

console.log(`--- booting Marlin for ${SIM_SECONDS}s of virtual time ---`);
const startedAt = Date.now();
while (cpu.cycles < totalCycles) {
  stepCpu(cpu);
}
console.log(`\n--- boot phase done (${Date.now() - startedAt}ms wall clock) ---`);

if (!serialOut.includes('Marlin')) {
  console.error('FAIL: no Marlin startup banner seen on USART0');
  process.exit(1);
}
console.log('OK: startup banner observed');

// Send M115 (firmware info query) and see if Marlin answers.
console.log('--- sending M115 ---');
serialOut = '';
const command = 'M115\n';
let charIndex = 0;
function pumpNextChar() {
  if (charIndex >= command.length) return;
  usart0.writeByte(command.charCodeAt(charIndex));
  charIndex++;
}
pumpNextChar();
usart0.onRxComplete = () => {
  pumpNextChar();
};

const m115Deadline = cpu.cycles + CYCLES_PER_SECOND * 2;
while (cpu.cycles < m115Deadline && !serialOut.includes('ok')) {
  stepCpu(cpu);
}

if (!serialOut.includes('FIRMWARE_NAME')) {
  console.error('FAIL: no FIRMWARE_NAME response to M115');
  process.exit(1);
}
console.log('\nOK: M115 answered correctly');
console.log('--- feasibility checkpoint PASSED ---');
