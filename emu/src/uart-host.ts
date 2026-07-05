import type { AVRUSART } from 'avr8js';

/**
 * Feeds gcode to Marlin over USART0 the way a real host (OctoPrint,
 * Pronterface, ...) does: one line at a time, waiting for Marlin's "ok"
 * before sending the next. Marlin's RX buffer is small, so blindly writing
 * a whole file would overrun it.
 */
export class UartHost {
  onLine?: (line: string) => void;

  private readonly pending: string[] = [];
  private currentLine = '';
  private charIndex = 0;
  private waitingForOk = false;
  private rxLineBuffer = '';

  constructor(private readonly usart: AVRUSART) {
    usart.onByteTransmit = (value) => this.handleTxByte(value);
    usart.onRxComplete = () => this.pumpNextChar();
  }

  /**
   * Marlin never sends "ok" for a comment-only or blank line -- it just
   * silently consumes it to keep the temperature ISR serviced (see
   * `process_line_done` in Marlin's queue.cpp, which returns early for an
   * "empty" line without calling `ok_to_send()`). Real hosts strip comments
   * client-side for exactly this reason, so we do too: sending one would
   * permanently stall our strict one-command-per-"ok" flow control.
   */
  enqueueGcode(text: string) {
    const lines = text
      .split(/\r?\n/)
      .map((l) => l.replace(/;.*$/, '').trim())
      .filter((l) => l.length > 0);
    this.pending.push(...lines);
    this.tryStartNext();
  }

  get linesRemaining(): number {
    return this.pending.length + (this.currentLine || this.waitingForOk ? 1 : 0);
  }

  private tryStartNext() {
    if (this.waitingForOk || this.currentLine || this.pending.length === 0) return;
    this.currentLine = this.pending.shift() + '\n';
    this.charIndex = 0;
    this.waitingForOk = true;
    this.pumpNextChar();
  }

  private pumpNextChar() {
    if (this.charIndex >= this.currentLine.length) {
      this.currentLine = '';
      return;
    }
    this.usart.writeByte(this.currentLine.charCodeAt(this.charIndex));
    this.charIndex++;
  }

  private handleTxByte(value: number) {
    const ch = String.fromCharCode(value);
    this.rxLineBuffer += ch;
    if (ch !== '\n') return;

    const line = this.rxLineBuffer;
    this.rxLineBuffer = '';
    this.onLine?.(line);

    if (line.trim() === 'ok' || line.trim().startsWith('ok ')) {
      this.waitingForOk = false;
      this.tryStartNext();
    }
  }
}
