export const enum LengthUnit { MICRON, MILLIMETER, CENTIMETER, INCH }

export function lengthUnitToAbbrev(unit: LengthUnit): string {
  switch (unit) {
    case LengthUnit.MICRON:
      return "µm";
    case LengthUnit.MILLIMETER:
      return "mm";
    case LengthUnit.CENTIMETER:
      return "cm";
    case LengthUnit.INCH:
      return "in";
    default:
      return null;
  }
}

export function lengthUnitToString(unit: LengthUnit): string {
  return unit.toString().toLowerCase();
}

export const MM_IN_CM: number = 10.0;
export const MM_IN_INCH: number = 25.4;
export const MM_IN_MICRON: number = 0.001;

export function convertLengthUnit(value: number, from: LengthUnit, to: LengthUnit): number {
  // mm
  let conversion = 0.0;
  if (from === to) {
    return value;
  }

  switch (from) {
    case LengthUnit.MICRON:
      conversion = value * MM_IN_MICRON;
      break;
    case LengthUnit.MILLIMETER:
      conversion = value;
      break;
    case LengthUnit.CENTIMETER:
      conversion = value * MM_IN_CM;
      break;
    case LengthUnit.INCH:
      conversion = value * MM_IN_INCH;
      break;
    default:
      break;
  }

  switch (to) {
    case LengthUnit.MICRON:
      conversion /= MM_IN_MICRON;
      break;
    case LengthUnit.MILLIMETER:
      break;
    case LengthUnit.CENTIMETER:
      conversion /= MM_IN_CM;
      break;
    case LengthUnit.INCH:
      conversion /= MM_IN_INCH;
      break;
    default:
      break;
  }

  return conversion;
}
