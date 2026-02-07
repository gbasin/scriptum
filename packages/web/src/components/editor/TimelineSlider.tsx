import clsx from "clsx";
import styles from "./TimelineSlider.module.css";

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
  const normalizedViewMode: HistoryViewMode =
    viewMode === "diff" ? "diff" : "authorship";

  return (
    <section aria-label="History timeline" className={styles.root} data-testid="history-timeline">
      <label className={styles.label} htmlFor="history-timeline-slider">
        History timeline
      </label>
      <fieldset
        aria-label="History view mode"
        className={styles.viewToggleGroup}
        data-testid="history-view-toggle"
      >
        <button
          aria-pressed={normalizedViewMode === "authorship"}
          className={clsx(
            styles.viewToggleButton,
            styles.viewToggleAuthorship,
            normalizedViewMode === "authorship" && styles.viewToggleAuthorshipActive,
          )}
          data-testid="history-view-toggle-authorship"
          onClick={() => onViewModeChange("authorship")}
          type="button"
        >
          Colored authorship
        </button>
        <button
          aria-pressed={normalizedViewMode === "diff"}
          className={clsx(
            styles.viewToggleButton,
            styles.viewToggleDiff,
            normalizedViewMode === "diff" && styles.viewToggleDiffActive,
          )}
          data-testid="history-view-toggle-diff"
          onClick={() => onViewModeChange("diff")}
          type="button"
        >
          Diff from current
        </button>
      </fieldset>
      <output
        aria-live="polite"
        className={styles.modeLabel}
        data-testid="history-view-mode-label"
      >
        View:{" "}
        {normalizedViewMode === "authorship"
          ? "Colored authorship"
          : "Diff from current"}
      </output>
      <input
        aria-label="History timeline slider"
        className={styles.slider}
        data-testid="history-timeline-slider"
        id="history-timeline-slider"
        max={clampedMax}
        min={0}
        onChange={(event) => {
          const nextValue = Number.parseInt(event.target.value, 10);
          onChange(clampTimelineValue(nextValue, clampedMax));
        }}
        step={1}
        type="range"
        value={clampedValue}
      />
      <output
        aria-live="polite"
        className={clsx(styles.positionLabel, styles.tabularNumbers)}
        data-testid="history-timeline-position"
      >
        Version {activeVersion}/{totalVersions}
      </output>
    </section>
  );
}
