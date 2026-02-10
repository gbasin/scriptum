import clsx from "clsx";
import { useMemo, useRef, useState } from "react";
import type { RuntimeMode } from "../../store/runtime";
import styles from "./OnboardingWizard.module.css";

const STEP_LABELS = [
  "Welcome",
  "Mode",
  "Workspace",
  "Template",
  "Editor intro",
] as const;

export type OnboardingMode = "local" | "team";
export type OnboardingTemplate = "blank" | "meeting-notes" | "spec-template";

export interface OnboardingResult {
  mode: OnboardingMode;
  template: OnboardingTemplate;
  workspaceDirectory: string | null;
  workspaceName: string;
}

interface TemplateOption {
  description: string;
  id: OnboardingTemplate;
  label: string;
}

interface OnboardingWizardProps {
  defaultWorkspaceName: string;
  desktopRuntime?: boolean;
  onComplete: (result: OnboardingResult) => void;
  onSkip: () => void;
  runtimeMode: RuntimeMode;
}

const TEMPLATE_OPTIONS: TemplateOption[] = [
  {
    id: "blank",
    label: "Blank",
    description: "Start with an empty markdown document.",
  },
  {
    id: "meeting-notes",
    label: "Meeting notes",
    description: "Agenda, notes, action items, and owners.",
  },
  {
    id: "spec-template",
    label: "Spec template",
    description: "Problem statement, goals, scope, and rollout plan.",
  },
];

function extractDirectoryName(files: FileList | null): string | null {
  if (!files || files.length === 0) {
    return null;
  }

  const firstFile = files.item(0);
  if (!firstFile) {
    return null;
  }

  const candidate = firstFile as File & { webkitRelativePath?: string };
  const relativePath = candidate.webkitRelativePath;
  if (typeof relativePath !== "string" || relativePath.length === 0) {
    return null;
  }

  const rootDirectory = relativePath.split("/")[0]?.trim();
  return rootDirectory && rootDirectory.length > 0 ? rootDirectory : null;
}

export function OnboardingWizard({
  defaultWorkspaceName,
  desktopRuntime = false,
  onComplete,
  onSkip,
  runtimeMode,
}: OnboardingWizardProps) {
  const [stepIndex, setStepIndex] = useState(0);
  const [mode, setMode] = useState<OnboardingMode>(
    runtimeMode === "local" ? "local" : "team",
  );
  const [workspaceName, setWorkspaceName] = useState(defaultWorkspaceName);
  const [workspaceDirectory, setWorkspaceDirectory] = useState("");
  const [template, setTemplate] = useState<OnboardingTemplate>("blank");
  const directoryPickerRef = useRef<HTMLInputElement | null>(null);

  const stepLabel = STEP_LABELS[stepIndex] ?? STEP_LABELS[0];
  const atLastStep = stepIndex === STEP_LABELS.length - 1;
  const canAdvance = stepIndex !== 2 || workspaceName.trim().length > 0;
  const selectedTemplate = useMemo(
    () => TEMPLATE_OPTIONS.find((option) => option.id === template),
    [template],
  );

  const openDirectoryPicker = () => {
    const input = directoryPickerRef.current;
    if (!input) {
      return;
    }

    input.setAttribute("webkitdirectory", "");
    input.setAttribute("directory", "");
    input.click();
  };

  const complete = () => {
    const name = workspaceName.trim();
    onComplete({
      mode,
      template,
      workspaceDirectory: workspaceDirectory.trim() || null,
      workspaceName: name.length > 0 ? name : defaultWorkspaceName,
    });
  };

  return (
    <section
      className={styles.shell}
      data-testid="onboarding-wizard"
      aria-label="First-run onboarding wizard"
    >
      <header className={styles.header}>
        <div className={styles.headerCopy}>
          <p className={styles.eyebrow}>First run</p>
          <h1 className={styles.title}>Welcome to Scriptum</h1>
        </div>
        <button
          className={clsx(styles.button, styles.secondaryButton)}
          data-testid="onboarding-skip-button"
          onClick={onSkip}
          type="button"
        >
          Skip
        </button>
      </header>

      <p className={styles.progress} data-testid="onboarding-step-progress">
        Step {stepIndex + 1} of {STEP_LABELS.length}: {stepLabel}
      </p>

      <div
        className={styles.card}
        data-testid={`onboarding-step-${stepIndex + 1}`}
      >
        {stepIndex === 0 ? (
          <div className={styles.content}>
            <h2 className={styles.sectionTitle}>Build local-first knowledge</h2>
            <p className={styles.copy}>
              Scriptum keeps your markdown local, syncs through git, and treats
              human and agent edits as first-class collaborators.
            </p>
            <ul className={styles.list}>
              <li>
                Real-time collaborative editing with CRDT conflict safety.
              </li>
              <li>Workspace-aware git sync and attribution trails.</li>
              <li>
                Presence, comments, history, and share flows in one editor.
              </li>
            </ul>
          </div>
        ) : null}

        {stepIndex === 1 ? (
          <div className={styles.content}>
            <h2 className={styles.sectionTitle}>Choose your starting mode</h2>
            <p className={styles.copy}>
              You can switch modes later in settings as your workflow evolves.
            </p>
            <div className={styles.choices}>
              <button
                className={clsx(styles.choice, {
                  [styles.choiceActive]: mode === "local",
                })}
                data-testid="onboarding-mode-local"
                onClick={() => setMode("local")}
                type="button"
              >
                <span className={styles.choiceTitle}>Local editing</span>
                <span className={styles.choiceMeta}>
                  Daemon-first workflow with no relay dependency.
                </span>
              </button>
              <button
                className={clsx(styles.choice, {
                  [styles.choiceActive]: mode === "team",
                })}
                data-testid="onboarding-mode-team"
                onClick={() => setMode("team")}
                type="button"
              >
                <span className={styles.choiceTitle}>Team collaboration</span>
                <span className={styles.choiceMeta}>
                  Relay + OAuth-backed collaboration with teammates.
                </span>
              </button>
            </div>
          </div>
        ) : null}

        {stepIndex === 2 ? (
          <div className={styles.content}>
            <h2 className={styles.sectionTitle}>Create your first workspace</h2>
            <label
              className={styles.fieldLabel}
              htmlFor="onboarding-workspace-name"
            >
              Workspace name
            </label>
            <input
              className={styles.textInput}
              data-testid="onboarding-workspace-name-input"
              id="onboarding-workspace-name"
              onChange={(event) => setWorkspaceName(event.target.value)}
              placeholder="My Workspace"
              type="text"
              value={workspaceName}
            />

            {mode === "local" && desktopRuntime ? (
              <div className={styles.directoryPanel}>
                <p className={styles.copy}>
                  Desktop mode can attach this workspace to a local directory.
                </p>
                <input
                  hidden
                  onChange={(event) => {
                    const directoryName = extractDirectoryName(
                      event.currentTarget.files,
                    );
                    if (directoryName) {
                      setWorkspaceDirectory(directoryName);
                    }
                    event.currentTarget.value = "";
                  }}
                  ref={directoryPickerRef}
                  type="file"
                />
                <div className={styles.directoryActions}>
                  <button
                    className={clsx(styles.button, styles.secondaryButton)}
                    data-testid="onboarding-directory-picker-button"
                    onClick={openDirectoryPicker}
                    type="button"
                  >
                    Choose directory
                  </button>
                  <span
                    className={styles.directoryValue}
                    data-testid="onboarding-directory-value"
                  >
                    {workspaceDirectory || "No directory selected"}
                  </span>
                </div>
              </div>
            ) : null}

            {mode === "local" && !desktopRuntime ? (
              <p
                className={styles.copy}
                data-testid="onboarding-auto-create-hint"
              >
                Web mode auto-creates the workspace in app storage.
              </p>
            ) : null}

            {mode === "team" ? (
              <p className={styles.copy}>
                Team mode expects relay connectivity and OAuth credentials.
              </p>
            ) : null}
          </div>
        ) : null}

        {stepIndex === 3 ? (
          <div className={styles.content}>
            <h2 className={styles.sectionTitle}>Create your first document</h2>
            <p className={styles.copy}>Pick a template to start faster.</p>
            <div className={styles.choices}>
              {TEMPLATE_OPTIONS.map((option) => (
                <button
                  key={option.id}
                  className={clsx(styles.choice, {
                    [styles.choiceActive]: template === option.id,
                  })}
                  data-testid={`onboarding-template-${option.id}`}
                  onClick={() => setTemplate(option.id)}
                  type="button"
                >
                  <span className={styles.choiceTitle}>{option.label}</span>
                  <span className={styles.choiceMeta}>
                    {option.description}
                  </span>
                </button>
              ))}
            </div>
          </div>
        ) : null}

        {stepIndex === 4 ? (
          <div className={styles.content}>
            <h2 className={styles.sectionTitle}>Editor quick tour</h2>
            <p className={styles.copy}>
              You&apos;ll land in the editor with these areas ready to explore:
            </p>
            <ul className={styles.list}>
              <li>Sidebar: workspace tree, tags, and quick search.</li>
              <li>Editor: markdown content with live collaboration.</li>
              <li>Status bar: sync, presence, and activity health.</li>
            </ul>
            <p className={styles.summary} data-testid="onboarding-summary">
              Workspace:{" "}
              <strong>{workspaceName.trim() || defaultWorkspaceName}</strong> ·
              Template: <strong>{selectedTemplate?.label ?? "Blank"}</strong>
            </p>
          </div>
        ) : null}
      </div>

      <footer className={styles.actions}>
        {stepIndex > 0 ? (
          <button
            className={clsx(styles.button, styles.secondaryButton)}
            data-testid="onboarding-back-button"
            onClick={() => setStepIndex((current) => Math.max(0, current - 1))}
            type="button"
          >
            Back
          </button>
        ) : (
          <span />
        )}

        {atLastStep ? (
          <button
            className={clsx(styles.button, styles.primaryButton)}
            data-testid="onboarding-complete-button"
            onClick={complete}
            type="button"
          >
            Open editor
          </button>
        ) : (
          <button
            className={clsx(styles.button, styles.primaryButton)}
            data-testid="onboarding-next-button"
            disabled={!canAdvance}
            onClick={() =>
              setStepIndex((current) =>
                Math.min(STEP_LABELS.length - 1, current + 1),
              )
            }
            type="button"
          >
            Continue
          </button>
        )}
      </footer>
    </section>
  );
}
