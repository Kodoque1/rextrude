/**
 * ATmega2560 interrupt vector table.
 *
 * avr8js's exported peripheral configs (timer0Config, usart0Config, etc.)
 * hardcode ATmega328P vector numbers. The 2560 has a much larger, differently
 * ordered vector table (57 vectors vs. 26), so every `*Interrupt` field must
 * be recomputed for this chip. Register addresses are unaffected -- those
 * happen to be shared across the classic-AVR family for the peripherals we
 * use, confirmed by cross-referencing avr-libc's iomxx0_1.h.
 *
 * Source of truth: avr-libc's `avr/iomxx0_1.h` (shared by the 640/1280/1281/
 * 2560/2561 family), which defines e.g. `#define TIMER0_COMPA_vect _VECTOR(21)`.
 * Verified empirically against `avr-objdump -d` of a compiled Marlin .elf:
 * vector N's jmp slot sits at byte address N*4 (2-word JMP, since flash > 8K),
 * i.e. word address N*2 -- and avr8js's `AVRInterruptConfig.address` is a
 * word address into `progMem`. Hence `vectorAddress(N) = N * 2`.
 */
export const VECTOR_NUMBER = {
  INT0: 1,
  INT1: 2,
  INT2: 3,
  INT3: 4,
  INT4: 5,
  INT5: 6,
  INT6: 7,
  INT7: 8,
  PCINT0: 9,
  PCINT1: 10,
  PCINT2: 11,
  WDT: 12,
  TIMER2_COMPA: 13,
  TIMER2_COMPB: 14,
  TIMER2_OVF: 15,
  TIMER1_CAPT: 16,
  TIMER1_COMPA: 17,
  TIMER1_COMPB: 18,
  TIMER1_COMPC: 19,
  TIMER1_OVF: 20,
  TIMER0_COMPA: 21,
  TIMER0_COMPB: 22,
  TIMER0_OVF: 23,
  SPI_STC: 24,
  USART0_RX: 25,
  USART0_UDRE: 26,
  USART0_TX: 27,
  ANALOG_COMP: 28,
  ADC: 29,
  EE_READY: 30,
  TIMER3_CAPT: 31,
  TIMER3_COMPA: 32,
  TIMER3_COMPB: 33,
  TIMER3_COMPC: 34,
  TIMER3_OVF: 35,
  USART1_RX: 36,
  USART1_UDRE: 37,
  USART1_TX: 38,
  TWI: 39,
  SPM_READY: 40,
  TIMER4_CAPT: 41,
  TIMER4_COMPA: 42,
  TIMER4_COMPB: 43,
  TIMER4_COMPC: 44,
  TIMER4_OVF: 45,
  TIMER5_CAPT: 46,
  TIMER5_COMPA: 47,
  TIMER5_COMPB: 48,
  TIMER5_COMPC: 49,
  TIMER5_OVF: 50,
  USART2_RX: 51,
  USART2_UDRE: 52,
  USART2_TX: 53,
  USART3_RX: 54,
  USART3_UDRE: 55,
  USART3_TX: 56,
} as const;

export type VectorName = keyof typeof VECTOR_NUMBER;

/** Word address of a vector's jmp slot, for use as an avr8js interrupt `address`. */
export function vaddr(name: VectorName): number {
  return VECTOR_NUMBER[name] * 2;
}
