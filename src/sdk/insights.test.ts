import { describe, expect, it } from "vitest";

import { heatLevels } from "./chartMath";
import { exportName, toCsv } from "./insights";

describe("toCsv", () => {
  it("quotes commas, quotes and line breaks, and ends with a newline", () => {
    expect(toCsv(["a", "b"], [["x,y", 'say "hi"'], ["line\nbreak", 3]])).toBe('a,b\r\n"x,y","say ""hi"""\r\n"line\nbreak",3\r\n');
  });
  it("writes empty cells for null and undefined", () => {
    expect(toCsv(["a", "b", "c"], [[null, undefined, 0]])).toBe("a,b,c\r\n,,0\r\n");
  });
  it("defuses cells a spreadsheet would run as a formula", () => {
    expect(toCsv(["n"], [["=1+1"], ["@SUM(A1)"], ["-5 days"], ["+1"]])).toBe("n\r\n'=1+1\r\n'@SUM(A1)\r\n'-5 days\r\n'+1\r\n");
    // Numbers are not text: a negative count stays a number.
    expect(toCsv(["n"], [[-5]])).toBe("n\r\n-5\r\n");
  });
});

describe("exportName", () => {
  it("makes a plain Windows-safe file name", () => {
    expect(exportName("Character presence", "csv")).toBe("character-presence.csv");
    expect(exportName("../x: y?", "svg")).toBe("x-y.svg");
    expect(exportName("", "png")).toBe("insight.png");
  });
});

describe("heatLevels", () => {
  it("splits positive values into four levels with strictly rising cuts", () => {
    expect(heatLevels([])).toEqual([1, 2, 3]);
    const c = heatLevels([0, 0, 1, 1, 1, 2, 5, 9, 40, 100]);
    expect(c[0]!).toBeLessThan(c[1]!);
    expect(c[1]!).toBeLessThan(c[2]!);
    expect(heatLevels([4, 4, 4, 4])).toEqual([4, 5, 6]);
  });
});
