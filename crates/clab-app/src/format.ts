/** Decimals shown per unit; other units fall back to three significant digits. */
const DECIMALS: Record<string, number> = {
  kg: 1,
  mm: 1,
  "%": 1,
  deg: 2,
  Hz: 2,
  "N/mm": 2,
  "Nm/deg": 2,
  "deg/mm": 4,
  "deg/deg": 4,
  "kg*mm^2": 0,
};

/** A metric value rounded for display; null (missing in the run) shows as "missing". */
export function formatValue(value: number | null, unit: string): string {
  if (value === null) return "missing";
  const decimals = DECIMALS[unit];
  return decimals === undefined ? String(Number(value.toPrecision(3))) : value.toFixed(decimals);
}
