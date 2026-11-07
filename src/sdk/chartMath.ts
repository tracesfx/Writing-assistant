/** Small pure helpers for the Insights charts. */
import type { ChartColors } from "./charts";

/** Thresholds splitting the positive values into four heat levels (quartiles). */
export function heatLevels(values: number[]): number[] {
  const v = values.filter((x) => x > 0).sort((a, b) => a - b);
  if (v.length === 0) return [1, 2, 3];
  const at = (q: number) => v[Math.min(v.length - 1, Math.floor(q * v.length))] ?? 1;
  const cuts = [at(0.25), at(0.5), at(0.75)];
  // Strictly increasing, so every piece has a range.
  for (let i = 1; i < cuts.length; i++) cuts[i] = Math.max(cuts[i]!, cuts[i - 1]! + 1);
  return cuts;
}

/** `visualMap` pieces for a 0 / four-level heat ramp. */
export function heatPieces(colors: ChartColors, cuts: number[]) {
  return [
    { value: 0, color: colors.heat[0] },
    { min: 1, max: cuts[0]! - 1, color: colors.heat[1] },
    { min: cuts[0]!, max: cuts[1]! - 1, color: colors.heat[2] },
    { min: cuts[1]!, max: cuts[2]! - 1, color: colors.heat[3] },
    { min: cuts[2]!, color: colors.heat[4] },
  ];
}
