import { avrInstruction } from 'avr8js';
import type { CPU } from 'avr8js';

const SREG_I_BIT = 0x80;

/**
 * Executes one instruction, with the one-instruction "delayed interrupt
 * enable" grace period real AVR hardware guarantees after any instruction
 * that sets SREG's I-bit (not just the dedicated `sei` opcode -- any write
 * to SREG that sets it, e.g. the `out SREG, r0` GCC emits when restoring a
 * saved status register).
 *
 * This matters because GCC's large-stack-frame epilogue looks like:
 *   cli; out SPH, r29; out SREG, r0; out SPL, r28
 * relying on that grace period so an interrupt can never fire between the
 * SPH and SPL writes (which would push a return address using a
 * half-updated stack pointer and corrupt the stack). avr8js's `cpu.tick()`
 * dispatches interrupts immediately based on the current SREG with no such
 * delay, which reproducibly crashed Marlin's stepper ISR epilogue during
 * real printing motion (see phase B notes). We can't patch avr8js itself,
 * so we simulate the delay from the outside: hide the I-bit from `tick()`'s
 * interrupt check for exactly one more instruction, then reveal it.
 */
export function stepCpu(cpu: CPU) {
  const sregBefore = cpu.data[95];
  avrInstruction(cpu);
  const sregAfter = cpu.data[95];
  const interruptsJustEnabled = !(sregBefore & SREG_I_BIT) && (sregAfter & SREG_I_BIT) !== 0;

  if (!interruptsJustEnabled) {
    cpu.tick();
    return;
  }

  cpu.data[95] = sregAfter & ~SREG_I_BIT; // hide I-bit so tick() won't dispatch yet
  cpu.tick(); // clock events still fire on schedule; interrupt dispatch is suppressed
  cpu.data[95] |= SREG_I_BIT; // reveal it again

  avrInstruction(cpu); // the mandatory grace instruction
  cpu.tick(); // interrupts may now dispatch, exactly one instruction late
}
