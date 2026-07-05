/**
 * Minimal Intel HEX parser -- just enough to load an avr-gcc/PlatformIO
 * .hex firmware image into a flash byte array. Supports record types
 * 00 (data), 01 (EOF), 02 (extended segment address) and 04 (extended
 * linear address); AVR toolchains only ever emit the last of those three.
 */
export function parseIntelHex(text: string, flashSize: number): Uint8Array {
  const flash = new Uint8Array(flashSize);
  let upperAddress = 0;

  for (const rawLine of text.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line) continue;
    if (line[0] !== ':') {
      throw new Error(`invalid Intel HEX record (missing ':'): ${line}`);
    }

    const bytes: number[] = [];
    for (let i = 1; i + 1 < line.length; i += 2) {
      bytes.push(parseInt(line.slice(i, i + 2), 16));
    }

    const byteCount = bytes[0];
    const address = (bytes[1] << 8) | bytes[2];
    const recordType = bytes[3];
    const data = bytes.slice(4, 4 + byteCount);

    switch (recordType) {
      case 0x00: {
        const base = upperAddress + address;
        for (let i = 0; i < data.length; i++) {
          flash[base + i] = data[i];
        }
        break;
      }
      case 0x01:
        return flash;
      case 0x02:
        upperAddress = ((data[0] << 8) | data[1]) << 4;
        break;
      case 0x04:
        upperAddress = ((data[0] << 8) | data[1]) << 16;
        break;
      default:
        // Ignore record types we don't need (03: start segment addr, 05: start linear addr).
        break;
    }
  }

  return flash;
}
