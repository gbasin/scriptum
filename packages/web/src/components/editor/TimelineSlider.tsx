export type HistoryViewMode = "authorship" | "diff";

export interface TimelineSliderProps {
  max: number;
  value: number;
  onChange: (nextValue: number) => void;
  viewMode: HistoryViewMode;
  onViewModeChange: (nextMode: HistoryViewMode) => void;
}

export function clampTimelineValue(value: number, max: number): number {
  if (!Number.isFinite(value)) {
    return 0;
  }
  const upperBound = Math.max(0, Math.floor(max));
  return Math.min(upperBound, Math.max(0, Math.floor(value)));
}

export function TimelineSlider({
  max,
  value,
  onChange,
  viewMode,
  onViewModeChange,
}: TimelineSliderProps) {
  const clampedMax = Math.max(0, max);
  const clampedValue = clampTimelineValue(value, clampedMax);
  const totalVersions = clampedMax + 1;
  const activeVersion = clampedValue + 1;
  const normalizedViewMode: HistoryViewMode = viewMode === "diff" ? "diff" : "authorship";

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
      <div
        aria-label="History view mode"
        data-testid="history-view-toggle"
        role="group"
        style={{ display: "flex", gap: "0.375rem", marginBottom: "0.375rem" }}
      >
        <button
          aria-pressed={normalizedViewMode === "authorship"}
          data-testid="history-view-toggle-authorship"
          onClick={() => onViewModeChange("authorship")}
          style={{
            background: normalizedViewMode === "authorship" ? "#dbeafe" : "#f3f4f6",
            border: "1px solid #93c5fd",
            borderRadius: "0.375rem",
            fontSize: "0.75rem",
            fontWeight: 600,
            padding: "0.2rem 0.5rem",
          }}
          type="button"
        >
          Colored authorship
        </button>
        <button
          aria-pressed={normalizedViewMode === "diff"}
          data-testid="history-view-toggle-diff"
          onClick={() => onViewModeChange("diff")}
          style={{
            background: normalizedViewMode === "diff" ? "#fee2e2" : "#f3f4f6",
            border: "1px solid #fca5a5",
            borderRadius: "0.375rem",
            fontSize: "0.75rem",
            fontWeight: 600,
            padding: "0.2rem 0.5rem",
          }}
          type="button"
        >
          Diff from current
        </button>
      </div>
      <output
        aria-live="polite"
        data-testid="history-view-mode-label"
        style={{ color: "#4b5563", display: "block", fontSize: "0.75rem", marginBottom: "0.25rem" }}
      >
        View: {normalizedViewMode === "authorship" ? "Colored authorship" : "Diff from current"}
      </output>
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
