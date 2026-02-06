export interface TimelineSliderProps {
  max: number;
  value: number;
  onChange: (nextValue: number) => void;
}

export function clampTimelineValue(value: number, max: number): number {
  if (!Number.isFinite(value)) {
    return 0;
  }
  const upperBound = Math.max(0, Math.floor(max));
  return Math.min(upperBound, Math.max(0, Math.floor(value)));
}

export function TimelineSlider({ max, value, onChange }: TimelineSliderProps) {
  const clampedMax = Math.max(0, max);
  const clampedValue = clampTimelineValue(value, clampedMax);
  const totalVersions = clampedMax + 1;
  const activeVersion = clampedValue + 1;

  return (
    <section
      aria-label="History timeline"
      data-testid="history-timeline"
      style={{
        borderTop: "1px solid #d1d5db",
        marginTop: "0.75rem",
        paddingTop: "0.5rem",
      }}
    >
      <label
        htmlFor="history-timeline-slider"
        style={{
          display: "block",
          fontSize: "0.8rem",
          fontWeight: 600,
          marginBottom: "0.375rem",
        }}
      >
        History timeline
      </label>
      <input
        aria-label="History timeline slider"
        data-testid="history-timeline-slider"
        id="history-timeline-slider"
        max={clampedMax}
        min={0}
        onChange={(event) => {
          const nextValue = Number.parseInt(event.target.value, 10);
          onChange(clampTimelineValue(nextValue, clampedMax));
        }}
        step={1}
        style={{ width: "100%" }}
        type="range"
        value={clampedValue}
      />
      <output
        aria-live="polite"
        data-testid="history-timeline-position"
        style={{ color: "#4b5563", display: "block", fontSize: "0.75rem", marginTop: "0.25rem" }}
      >
        Version {activeVersion}/{totalVersions}
      </output>
    </section>
  );
}

