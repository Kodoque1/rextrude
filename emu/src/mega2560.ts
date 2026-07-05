import {
  AVRADC,
  AVRClock,
  AVREEPROM,
  AVRIOPort,
  AVRTimer,
  AVRUSART,
  AVRWatchdog,
  CPU,
  EEPROMMemoryBackend,
  adcConfig,
  eepromConfig,
  portAConfig,
  portBConfig,
  portCConfig,
  portDConfig,
  portEConfig,
  portFConfig,
  portGConfig,
  portHConfig,
  portJConfig,
  portKConfig,
  portLConfig,
  timer0Config,
  timer1Config,
  timer2Config,
  usart0Config,
  watchdogConfig,
  type AVRTimerConfig,
} from 'avr8js';

type USARTConfig = typeof usart0Config;

import { atmega2560Channels } from './adc-mega.js';
import { vaddr } from './vectors.js';

export const MEGA2560_FREQ_HZ = 16_000_000;

/**
 * ATmega2560's low+extended I/O space is 512 bytes (0x000-0x1FF), twice the
 * 256 bytes avr8js's CPU assumes by default (it was written for the
 * ATmega328P's 256-byte I/O space, and unconditionally sizes SRAM to start
 * right after that fixed 256-byte block). To end up with a `data` array
 * whose real usable SRAM actually starts at the 2560's true 0x200 and runs
 * for the full 8192 bytes to RAMEND (0x21FF, per avr-libc's iom2560.h), we
 * have to pad `sramBytes` by the missing 256 bytes. Getting this wrong means
 * the last 256 bytes of stack space silently write past the backing
 * Uint8Array and vanish -- typically corrupting the stack within the first
 * few dozen pushes after boot.
 */
const MEGA2560_SRAM_BYTES = 8192 + 256;

export interface Mega2560 {
  cpu: CPU;
  ports: Record<'A' | 'B' | 'C' | 'D' | 'E' | 'F' | 'G' | 'H' | 'J' | 'K' | 'L', AVRIOPort>;
  timer0: AVRTimer;
  timer1: AVRTimer;
  timer2: AVRTimer;
  usart0: AVRUSART;
  adc: AVRADC;
  eeprom: AVREEPROM;
  watchdog: AVRWatchdog;
}

function remapTimer(
  config: AVRTimerConfig,
  vectors: Partial<Record<'capt' | 'compA' | 'compB' | 'compC' | 'ovf', number>>,
): AVRTimerConfig {
  return {
    ...config,
    captureInterrupt: vectors.capt ?? 0,
    compAInterrupt: vectors.compA ?? 0,
    compBInterrupt: vectors.compB ?? 0,
    compCInterrupt: vectors.compC ?? 0,
    ovfInterrupt: vectors.ovf ?? 0,
  };
}

function remapUsart(config: USARTConfig, rx: number, udre: number, tx: number): USARTConfig {
  return {
    ...config,
    rxCompleteInterrupt: rx,
    dataRegisterEmptyInterrupt: udre,
    txCompleteInterrupt: tx,
  };
}

/**
 * Builds an ATmega2560 CPU wired up with the peripherals Marlin's stock
 * RAMPS 1.4 build actually touches: all 11 GPIO ports (A-L), Timer0/1/2,
 * USART0, the ADC (with the 2560's 16 channels), EEPROM (4KB) and the
 * watchdog. Timer3/4/5 and USART1-3 are intentionally not wired up --
 * Marlin's default config (no servos, single serial, soft-PWM heaters)
 * never touches them; see the phase B plan notes for the reasoning.
 */
export function createMega2560(progMem: Uint16Array): Mega2560 {
  const cpu = new CPU(progMem, MEGA2560_SRAM_BYTES);

  const ports = {
    A: new AVRIOPort(cpu, portAConfig),
    B: new AVRIOPort(cpu, portBConfig),
    C: new AVRIOPort(cpu, portCConfig),
    D: new AVRIOPort(cpu, portDConfig),
    E: new AVRIOPort(cpu, portEConfig),
    F: new AVRIOPort(cpu, portFConfig),
    G: new AVRIOPort(cpu, portGConfig),
    H: new AVRIOPort(cpu, portHConfig),
    J: new AVRIOPort(cpu, portJConfig),
    K: new AVRIOPort(cpu, portKConfig),
    L: new AVRIOPort(cpu, portLConfig),
  };

  const timer0 = new AVRTimer(
    cpu,
    remapTimer(timer0Config, {
      compA: vaddr('TIMER0_COMPA'),
      compB: vaddr('TIMER0_COMPB'),
      ovf: vaddr('TIMER0_OVF'),
    }),
  );
  const timer1 = new AVRTimer(
    cpu,
    remapTimer(timer1Config, {
      capt: vaddr('TIMER1_CAPT'),
      compA: vaddr('TIMER1_COMPA'),
      compB: vaddr('TIMER1_COMPB'),
      compC: vaddr('TIMER1_COMPC'),
      ovf: vaddr('TIMER1_OVF'),
    }),
  );
  const timer2 = new AVRTimer(
    cpu,
    remapTimer(timer2Config, {
      compA: vaddr('TIMER2_COMPA'),
      compB: vaddr('TIMER2_COMPB'),
      ovf: vaddr('TIMER2_OVF'),
    }),
  );

  const usart0 = new AVRUSART(
    cpu,
    remapUsart(usart0Config, vaddr('USART0_RX'), vaddr('USART0_UDRE'), vaddr('USART0_TX')),
    MEGA2560_FREQ_HZ,
  );

  const adc = new AVRADC(cpu, {
    ...adcConfig,
    adcInterrupt: vaddr('ADC'),
    numChannels: 16,
    muxInputMask: 0x3f,
    muxChannels: atmega2560Channels,
  });

  const eeprom = new AVREEPROM(cpu, new EEPROMMemoryBackend(4096), {
    ...eepromConfig,
    eepromReadyInterrupt: vaddr('EE_READY'),
  });

  const clock = new AVRClock(cpu, MEGA2560_FREQ_HZ);
  const watchdog = new AVRWatchdog(
    cpu,
    { ...watchdogConfig, watchdogInterrupt: vaddr('WDT') },
    clock,
  );

  return { cpu, ports, timer0, timer1, timer2, usart0, adc, eeprom, watchdog };
}
