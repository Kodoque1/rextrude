import { ADCMuxInputType, type ADCMuxConfiguration } from 'avr8js';

/**
 * ATmega2560 has 16 single-ended ADC channels (ADC0-15) instead of the
 * ATmega328P's 8. Channels 8-15 are selected by setting ADCSRB's MUX5 bit
 * in addition to ADMUX's MUX[4:0] bits; avr8js's generic AVRADC already
 * folds MUX5 into the lookup key as `0x20 | (ADMUX & 0x1f)` (see adc.js),
 * so channel 8 shows up at key 0x20, channel 15 at key 0x27.
 */
export const atmega2560Channels: ADCMuxConfiguration = {
  0: { type: ADCMuxInputType.SingleEnded, channel: 0 },
  1: { type: ADCMuxInputType.SingleEnded, channel: 1 },
  2: { type: ADCMuxInputType.SingleEnded, channel: 2 },
  3: { type: ADCMuxInputType.SingleEnded, channel: 3 },
  4: { type: ADCMuxInputType.SingleEnded, channel: 4 },
  5: { type: ADCMuxInputType.SingleEnded, channel: 5 },
  6: { type: ADCMuxInputType.SingleEnded, channel: 6 },
  7: { type: ADCMuxInputType.SingleEnded, channel: 7 },
  0x20: { type: ADCMuxInputType.SingleEnded, channel: 8 },
  0x21: { type: ADCMuxInputType.SingleEnded, channel: 9 },
  0x22: { type: ADCMuxInputType.SingleEnded, channel: 10 },
  0x23: { type: ADCMuxInputType.SingleEnded, channel: 11 },
  0x24: { type: ADCMuxInputType.SingleEnded, channel: 12 },
  0x25: { type: ADCMuxInputType.SingleEnded, channel: 13 },
  0x26: { type: ADCMuxInputType.SingleEnded, channel: 14 },
  0x27: { type: ADCMuxInputType.SingleEnded, channel: 15 },
  0x1e: { type: ADCMuxInputType.Constant, voltage: 1.1 },
  0x1f: { type: ADCMuxInputType.Constant, voltage: 0 },
};
