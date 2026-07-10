/**
 * The subset of `AVRUSART` this class needs -- kept minimal so tests can
 * drive it with a plain fake instead of spinning up a real avr8js CPU.
 */
export interface UartLike {
  onByteTransmit?: ((value: number) => void) | null;
  onRxComplete?: (() => void) | null;
  writeByte(value: number): boolean | undefined;
}

/**
 * Feeds gcode to Marlin over USART0 the way a real host (OctoPrint,
 * Pronterface, ...) does: one line at a time, waiting for Marlin's "ok"
 * before sending the next. Marlin's RX buffer is small, so blindly writing
 * a whole file would overrun it.
 */
export class UartHost {
  onLine?: (line: string) => void;

  /** Raw gcode texts queued by enqueueGcode, consumed front-first. */
  private readonly chunks: string[] = [];
  /** Char offset of the next unread char within chunks[0]. */
  private scanPos = 0;
  private currentLine = '';
  private charIndex = 0;
  private waitingForOk = false;
  private rxLineBuffer = '';

  constructor(private readonly usart: UartLike) {
    usart.onByteTransmit = (value) => this.handleTxByte(value);
    usart.onRxComplete = () => this.pumpNextChar();
  }

  /**
   * Queues raw gcode text to stream. Scanning into lines is lazy (see
   * `nextCommandLine`), so this never allocates an array proportional to the
   * file's line count -- important for multi-million-line files, where an
   * eager `split` + spread into `pending` would both stall the main thread
   * for seconds and risk a `RangeError` from spreading millions of args.
   */
  enqueueGcode(text: string) {
    if (text.length === 0) return;
    this.chunks.push(text);
    this.tryStartNext();
  }

  /** True while a command is in flight or unscanned text remains queued. */
  get hasPending(): boolean {
    return this.waitingForOk || this.currentLine !== '' || this.chunks.length > 0;
  }

  /**
   * Marlin never sends "ok" for a comment-only or blank line -- it just
   * silently consumes it to keep the temperature ISR serviced (see
   * `process_line_done` in Marlin's queue.cpp, which returns early for an
   * "empty" line without calling `ok_to_send()`). Real hosts strip comments
   * client-side for exactly this reason, so we do too: sending one would
   * permanently stall our strict one-command-per-"ok" flow control.
   *
   * Scans forward through the queued chunks for the next non-empty,
   * comment-stripped line, discarding exhausted chunks as it goes. A chunk
   * boundary terminates a line even without a trailing newline, matching the
   * old per-call `split` behavior for multiple `enqueueGcode` calls.
   */
  private nextCommandLine(): string | null {
    while (this.chunks.length > 0) {
      const text = this.chunks[0];
      if (this.scanPos >= text.length) {
        this.chunks.shift();
        this.scanPos = 0;
        continue;
      }
      const nl = text.indexOf('\n', this.scanPos);
      const end = nl === -1 ? text.length : nl;
      let line = text.slice(this.scanPos, end);
      this.scanPos = end + 1; // past the '\n' (or past end-of-text)
      const semi = line.indexOf(';');
      if (semi !== -1) line = line.slice(0, semi);
      line = line.trim(); // also drops the '\r' of CRLF files
      if (line.length > 0) return line;
    }
    return null;
  }

  private tryStartNext() {
    if (this.waitingForOk || this.currentLine) return;
    const line = this.nextCommandLine();
    if (line === null) return;
    this.currentLine = line + '\n';
    this.charIndex = 0;
    this.waitingForOk = true;
    this.pumpNextChar();
  }

  private pumpNextChar() {
    if (this.charIndex >= this.currentLine.length) {
      this.currentLine = '';
      return;
    }
    // Advance charIndex *before* writeByte: a real AVRUSART fires
    // onRxComplete asynchronously (via CPU cycle stepping), but nothing
    // stops a caller (e.g. a synchronous test fake) from re-entering
    // pumpNextChar from within writeByte. Incrementing first keeps that
    // reentrant call reading the next index instead of resending this char
    // forever.
    const idx = this.charIndex;
    this.charIndex++;
    this.usart.writeByte(this.currentLine.charCodeAt(idx));
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
