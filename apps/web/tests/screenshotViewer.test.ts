import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { ScreenshotViewer, stepIndex } from "../src/components/ScreenshotViewer";

describe("ScreenshotViewer", () => {
  it("allows native pinch zoom alongside gallery swipes and explains the mobile gestures", () => {
    const markup = renderToStaticMarkup(
      createElement(ScreenshotViewer, {
        shots: [{ src: "/screenshot.webp", alt: "Table preview", caption: "Browse rows" }],
        index: 0,
        onIndexChange: () => {},
        onClose: () => {},
      }),
    );

    expect(markup).toMatch(/class="[^"]*touch-pan-y touch-pinch-zoom[^"]*"/);
    expect(markup).toContain("Pinch to zoom");
  });
});

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
