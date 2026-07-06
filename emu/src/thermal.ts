/**
 * A deliberately simple thermal model: just enough for M109/M190 to unblock
 * at a plausible rate and for the temperature to visibly climb/settle, not
 * a faithful reproduction of Marlin's internal thermistor lookup table.
 *
 * Each heater is a first-order RC-style system: power in from the (duty
 * fraction x max wattage), heat lost proportionally to the gap above
 * ambient, integrated into a thermal mass. The resulting temperature is
 * converted to a thermistor voltage via the standard NTC beta equation for
 * a 100k/beta-4092 thermistor (Marlin's common "table 1"), which is what
 * `RampsBoard`'s wired-up ADC channels expect to read.
 */

const ABSOLUTE_ZERO_C = -273.15;

export interface HeaterParams {
  ambientC: number;
  maxPowerW: number;
  thermalMassJPerC: number;
  /** Heat loss to ambient, in watts per degree C above ambient. */
  lossWPerC: number;
}

export class HeaterModel {
  celsius: number;

  constructor(private readonly params: HeaterParams) {
    this.celsius = params.ambientC;
  }

  step(dtSeconds: number, duty: number) {
    const { maxPowerW, lossWPerC, thermalMassJPerC, ambientC } = this.params;
    const powerIn = duty * maxPowerW;
    const powerOut = lossWPerC * (this.celsius - ambientC);
    this.celsius += ((powerIn - powerOut) / thermalMassJPerC) * dtSeconds;
  }
}

/** 100k NTC thermistor with a 4.7k pullup to 5V, Marlin's common "table 1" curve family. */
export function ntcVoltage(
  celsius: number,
  r25 = 100_000,
  beta = 4092,
  pullup = 4700,
  vcc = 5,
): number {
  const t = celsius - ABSOLUTE_ZERO_C;
  const t25 = 25 - ABSOLUTE_ZERO_C;
  const resistance = r25 * Math.exp(beta * (1 / t - 1 / t25));
  return (vcc * resistance) / (resistance + pullup);
}

const HOTEND_PARAMS: HeaterParams = {
  ambientC: 25,
  maxPowerW: 40,
  thermalMassJPerC: 6,
  // Chosen so full-duty equilibrium (ambient + maxPower/lossWPerC) sits
  // comfortably above typical PLA/PETG/ABS hotend targets (~180-250C).
  lossWPerC: 0.14,
};

const BED_PARAMS: HeaterParams = {
  ambientC: 25,
  maxPowerW: 120,
  thermalMassJPerC: 130,
  lossWPerC: 1.7,
};

export class ThermalSim {
  readonly hotend = new HeaterModel(HOTEND_PARAMS);
  readonly bed = new HeaterModel(BED_PARAMS);

  /** Advances both heaters and writes the resulting voltages into the ADC channels. */
  step(
    dtSeconds: number,
    duty: { hotend: number; bed: number },
    adc: { channelValues: number[] },
    channels: { hotend: number; bed: number },
  ) {
    this.hotend.step(dtSeconds, duty.hotend);
    this.bed.step(dtSeconds, duty.bed);
    adc.channelValues[channels.hotend] = ntcVoltage(this.hotend.celsius);
    adc.channelValues[channels.bed] = ntcVoltage(this.bed.celsius);
  }
}
