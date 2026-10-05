import { stepIndex } from "../src/components/ScreenshotViewer";

describe("stepIndex", () => {
  it("moves forward and back", () => {
    expect(stepIndex(1, 1, 5)).toBe(2);
    expect(stepIndex(1, -1, 5)).toBe(0);
  });

  it("wraps around both ends", () => {
    expect(stepIndex(4, 1, 5)).toBe(0);
    expect(stepIndex(0, -1, 5)).toBe(4);
  });
});
