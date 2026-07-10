/**
 * Checkpoint: exercise `UartHost` in isolation (no real AVR CPU) against a
 * fake `UartLike` that echoes RX completion synchronously per byte.
 *
 * Regression coverage for the lazy line-cursor rewrite: `enqueueGcode` used
 * to eagerly `split` the whole input into an array and `pending.push(...lines)`
 * it -- which throws a `RangeError` in V8 once the array gets into the
 * hundreds of thousands of elements (a real 2.8M-line gcode file hit this
 * every time). Scenario 1 proves a large input streams through without
 * throwing or hanging, in bounded time, with correct comment/blank stripping
 * and ordering. Scenarios 2-3 pin down the surrounding semantics (multi-chunk
 * enqueue, comments-only chunks, strict one-command-per-"ok" flow control)
 * that the rewrite must preserve.
 */
import { UartHost, type UartLike } from './uart-host.js';

interface Harness {
  host: UartHost;
  received: string[];
  firmwareSendsOk: () => void;
}

function fail(msg: string): never {
  console.error(`FAIL: ${msg}`);
  process.exit(1);
}

/** Also sidesteps TS narrowing `received.length` to a literal across calls. */
function expectLength(received: readonly string[], expected: number, what: string) {
  const actual: number = received.length;
  if (actual !== expected) {
    fail(`${what}: got ${actual}, expected ${expected}`);
  }
}

function createHarness(): Harness {
  const received: string[] = [];
  let lineBuf = '';
  const fake: UartLike = {
    writeByte(value: number): boolean {
      const ch = String.fromCharCode(value);
      if (ch === '\n') {
        received.push(lineBuf);
        lineBuf = '';
      } else {
        lineBuf += ch;
      }
      this.onRxComplete?.();
      return true;
    },
  };
  const host = new UartHost(fake);
  function firmwareSendsOk() {
    for (const ch of 'ok\n') fake.onByteTransmit?.(ch.charCodeAt(0));
  }
  return { host, received, firmwareSendsOk };
}

console.log('--- scenario 1: large synthetic file does not crash or hang ---');
{
  const LINE_COUNT = 1_000_000;
  const lines: string[] = new Array(LINE_COUNT);
  const expected: string[] = [];
  for (let i = 0; i < LINE_COUNT; i++) {
    switch (i % 5) {
      case 0: {
        const cmd = `G1 X${i} Y${i} E${i}`;
        lines[i] = cmd;
        expected.push(cmd);
        break;
      }
      case 1:
        lines[i] = `; pure comment ${i}`;
        break;
      case 2:
        lines[i] = '';
        break;
      case 3:
        lines[i] = `G1 Z0.2 ; trailing comment ${i}`;
        expected.push('G1 Z0.2');
        break;
      case 4:
        // '\r' before the '\n' join point simulates a CRLF-terminated line.
        lines[i] = 'G1 F1500\r';
        expected.push('G1 F1500');
        break;
    }
  }
  const bigText = lines.join('\n');
  console.log(
    `generated ${LINE_COUNT} lines (${(bigText.length / 1e6).toFixed(1)}MB), expecting ${expected.length} commands`,
  );

  const h = createHarness();
  const t0 = performance.now();
  try {
    h.host.enqueueGcode(bigText);
  } catch (err) {
    fail(`enqueueGcode threw on a large input (RangeError regression): ${err}`);
  }

  const maxIterations = expected.length + 10;
  let iterations = 0;
  while (h.host.hasPending && iterations < maxIterations) {
    h.firmwareSendsOk();
    iterations++;
  }
  const elapsedMs = performance.now() - t0;

  if (h.host.hasPending) fail(`did not drain within ${maxIterations} oks -- likely hung`);
  expectLength(h.received, expected.length, 'total commands received');
  for (let i = 0; i < expected.length; i++) {
    if (h.received[i] !== expected[i]) {
      fail(
        `command ${i} mismatch: got ${JSON.stringify(h.received[i])}, expected ${JSON.stringify(expected[i])}`,
      );
    }
  }
  if (elapsedMs >= 15000) fail(`took ${elapsedMs.toFixed(0)}ms, expected under 15000ms`);
  console.log(`OK: ${expected.length} commands streamed and verified in ${elapsedMs.toFixed(0)}ms`);
}

console.log('\n--- scenario 2: multiple enqueueGcode calls ---');
{
  const h = createHarness();

  // A chunk with no trailing newline must still be flushed (chunk boundary
  // terminates the line), not left stuck waiting for more text.
  h.host.enqueueGcode('G1 X1');
  h.firmwareSendsOk();
  expectLength(h.received, 1, 'chunk without trailing newline');
  if (h.received[0] !== 'G1 X1') {
    fail(`chunk without trailing newline mishandled: ${JSON.stringify(h.received)}`);
  }

  // A comments-only/blank chunk must send nothing and leave the host idle.
  const beforeCommentsOnly: number = h.received.length;
  h.host.enqueueGcode('; just a comment\n\n; another\n');
  if (h.host.hasPending) fail('a comments-only/blank chunk must not leave hasPending true');
  expectLength(
    h.received,
    beforeCommentsOnly,
    'a comments-only/blank chunk must not send any command',
  );

  // A normal multi-line chunk streams in order.
  h.host.enqueueGcode('G1 X2\nG1 X3\n');
  h.firmwareSendsOk();
  h.firmwareSendsOk();
  const tail = h.received.slice(-2);
  if (tail[0] !== 'G1 X2' || tail[1] !== 'G1 X3') {
    fail(`multi-line chunk order/content wrong: ${JSON.stringify(tail)}`);
  }
  if (h.host.hasPending) fail('host should be idle after draining scenario 2');
}
console.log('OK: multiple enqueueGcode calls handled correctly');

console.log('\n--- scenario 3: strict one-command-per-ok flow control ---');
{
  const h = createHarness();
  h.host.enqueueGcode('G1 X1\nG1 X2\nG1 X3\n');
  expectLength(h.received, 1, 'only the first command should be sent before any ok');
  h.firmwareSendsOk();
  expectLength(h.received, 2, 'exactly one more command should be released per ok');
  h.firmwareSendsOk();
  expectLength(h.received, 3, 'exactly one more command should be released per ok');
  if (!h.host.hasPending) fail('host should still be waiting on an ok for the third command');
  h.firmwareSendsOk();
  if (h.host.hasPending) fail('host should be idle after the final ok in scenario 3');
}
console.log('OK: flow control is strictly one command per ok');

console.log('\n--- uart-host checkpoint PASSED ---');
