import { renderToString } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { clampTimelineValue, TimelineSlider } from "./TimelineSlider";

describe("TimelineSlider", () => {
  it("renders timeline slider metadata for the current version window", () => {
    const html = renderToString(
      <TimelineSlider
        max={5}
        onChange={() => {
          // no-op for server-side render test
        }}
        value={2}
      />
    );
    const normalized = html.replaceAll("<!-- -->", "");

    expect(normalized).toContain("History timeline");
    expect(normalized).toContain('type="range"');
    expect(normalized).toContain('max="5"');
    expect(normalized).toContain('value="2"');
    expect(normalized).toContain("Version 3/6");
  });

  it("clamps timeline values into [0, max]", () => {
    expect(clampTimelineValue(-5, 10)).toBe(0);
    expect(clampTimelineValue(3, 10)).toBe(3);
    expect(clampTimelineValue(99, 10)).toBe(10);
    expect(clampTimelineValue(Number.NaN, 10)).toBe(0);
  });
});

