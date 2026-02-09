import type { Document } from "@scriptum/shared";
import type { InlineCommentThread } from "../../lib/inline-comments";
import type { RightPanelTab } from "../../store/ui";
import styles from "../Layout.module.css";
import type { BacklinkEntry } from "../right-panel/Backlinks";
import { Backlinks } from "../right-panel/Backlinks";
import { CommentsPanel } from "../right-panel/CommentsPanel";
import { Outline } from "../right-panel/Outline";
import { RIGHT_PANEL_TAB_IDS, RIGHT_PANEL_TAB_PANEL_IDS } from "./layoutUtils";

export interface LayoutRightPanelProps {
  activeDocumentId: string | null;
  activeWorkspaceId: string | null;
  incomingBacklinkEntries: readonly BacklinkEntry[];
  outlineContainer: HTMLElement | null;
  rightPanelOpen: boolean;
  rightPanelTab: RightPanelTab;
  showOutlineSkeleton: boolean;
  showPanelSkeletons: boolean;
  threadsByDocumentKey: Record<string, InlineCommentThread[]>;
  workspaceDocuments: readonly Document[];
  onBacklinkSelect: (documentId: string) => void;
  onCommentThreadSelect: (documentId: string, threadId: string) => void;
  onRightPanelTabChange: (tab: RightPanelTab) => void;
  onToggleRightPanel: () => void;
}

function panelTabClassName(
  rightPanelTab: RightPanelTab,
  tab: RightPanelTab,
): string {
  return rightPanelTab === tab
    ? styles.panelTabButtonActive
    : styles.panelTabButton;
}

export function LayoutRightPanel({
  activeDocumentId,
  activeWorkspaceId,
  incomingBacklinkEntries,
  outlineContainer,
  rightPanelOpen,
  rightPanelTab,
  showOutlineSkeleton,
  showPanelSkeletons,
  threadsByDocumentKey,
  workspaceDocuments,
  onBacklinkSelect,
  onCommentThreadSelect,
  onRightPanelTabChange,
  onToggleRightPanel,
}: LayoutRightPanelProps) {
  if (!rightPanelOpen) {
    return (
      <button
        aria-label="Show document outline panel"
        className={styles.showOutlineButton}
        data-testid="outline-panel-toggle"
        onClick={onToggleRightPanel}
        type="button"
      >
        Show Outline
      </button>
    );
  }

  return (
    <aside
      aria-label="Document outline panel"
      className={styles.outlinePanel}
      data-motion="enter"
      data-testid="outline-panel"
    >
      <div className={styles.panelHeader}>
        <h2 className={styles.panelHeading}>Outline</h2>
        <button
          className={styles.secondaryButton}
          data-testid="outline-panel-toggle"
          onClick={onToggleRightPanel}
          type="button"
        >
          Hide
        </button>
      </div>
      <div
        aria-label="Right panel tabs"
        className={styles.panelTabs}
        role="tablist"
      >
        <button
          aria-controls={RIGHT_PANEL_TAB_PANEL_IDS.outline}
          aria-selected={rightPanelTab === "outline"}
          className={panelTabClassName(rightPanelTab, "outline")}
          data-testid={RIGHT_PANEL_TAB_IDS.outline}
          id={RIGHT_PANEL_TAB_IDS.outline}
          onClick={() => onRightPanelTabChange("outline")}
          role="tab"
          tabIndex={rightPanelTab === "outline" ? 0 : -1}
          type="button"
        >
          Outline
        </button>
        <button
          aria-controls={RIGHT_PANEL_TAB_PANEL_IDS.backlinks}
          aria-selected={rightPanelTab === "backlinks"}
          className={panelTabClassName(rightPanelTab, "backlinks")}
          data-testid={RIGHT_PANEL_TAB_IDS.backlinks}
          id={RIGHT_PANEL_TAB_IDS.backlinks}
          onClick={() => onRightPanelTabChange("backlinks")}
          role="tab"
          tabIndex={rightPanelTab === "backlinks" ? 0 : -1}
          type="button"
        >
          Backlinks
        </button>
        <button
          aria-controls={RIGHT_PANEL_TAB_PANEL_IDS.comments}
          aria-selected={rightPanelTab === "comments"}
          className={panelTabClassName(rightPanelTab, "comments")}
          data-testid={RIGHT_PANEL_TAB_IDS.comments}
          id={RIGHT_PANEL_TAB_IDS.comments}
          onClick={() => onRightPanelTabChange("comments")}
          role="tab"
          tabIndex={rightPanelTab === "comments" ? 0 : -1}
          type="button"
        >
          Comments
        </button>
      </div>

      {rightPanelTab === "outline" ? (
        <section
          aria-labelledby={RIGHT_PANEL_TAB_IDS.outline}
          data-testid="right-panel-tabpanel-outline"
          id={RIGHT_PANEL_TAB_PANEL_IDS.outline}
          role="tabpanel"
        >
          <Outline
            editorContainer={outlineContainer}
            loading={showOutlineSkeleton}
          />
        </section>
      ) : null}

      {rightPanelTab === "backlinks" ? (
        <section
          aria-label="Incoming backlinks"
          aria-labelledby={RIGHT_PANEL_TAB_IDS.backlinks}
          className={styles.backlinksSection}
          data-testid="backlinks-panel"
          id={RIGHT_PANEL_TAB_PANEL_IDS.backlinks}
          role="tabpanel"
        >
          <Backlinks
            backlinks={incomingBacklinkEntries.slice()}
            documentId={activeDocumentId ?? ""}
            loading={showPanelSkeletons}
            onBacklinkSelect={onBacklinkSelect}
            workspaceId={activeWorkspaceId ?? ""}
          />
        </section>
      ) : null}

      {rightPanelTab === "comments" ? (
        <section
          aria-labelledby={RIGHT_PANEL_TAB_IDS.comments}
          data-testid="right-panel-tabpanel-comments"
          id={RIGHT_PANEL_TAB_PANEL_IDS.comments}
          role="tabpanel"
        >
          <CommentsPanel
            activeDocumentId={activeDocumentId}
            documents={workspaceDocuments}
            onThreadSelect={onCommentThreadSelect}
            threadsByDocumentKey={threadsByDocumentKey}
            workspaceId={activeWorkspaceId}
          />
        </section>
      ) : null}
    </aside>
  );
}
