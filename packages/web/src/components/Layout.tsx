import { Outlet } from "react-router-dom";
import { useLayoutController } from "../hooks/useLayoutController";
import type { IncomingBacklink } from "../lib/wiki-links";
import { ErrorBoundary } from "./ErrorBoundary";
import styles from "./Layout.module.css";
import { LayoutDialogs } from "./layout/LayoutDialogs";
import { LayoutRightPanel } from "./layout/LayoutRightPanel";
import { LayoutSidebar } from "./layout/LayoutSidebar";
import { isNewDocumentShortcut, formatRenameBacklinkToast } from "./layout/layoutUtils";
import { ToastViewport } from "./ToastViewport";

export type { IncomingBacklink };
export { isNewDocumentShortcut, formatRenameBacklinkToast };

export function Layout() {
  const {
    dialogs,
    handleCompactPanelBackdropClick,
    handleEditorAreaRef,
    rightPanel,
    showCompactPanelBackdrop,
    sidebar,
  } = useLayoutController();

  return (
    <div className={styles.layout} data-testid="app-layout">
      {showCompactPanelBackdrop ? (
        <button
          aria-label="Close panels"
          className={styles.compactPanelBackdrop}
          data-testid="compact-panel-backdrop"
          onClick={handleCompactPanelBackdropClick}
          type="button"
        />
      ) : null}

      <LayoutSidebar {...sidebar} />

      <main
        aria-label="Editor area"
        className={styles.editorArea}
        data-testid="app-editor-area"
        ref={handleEditorAreaRef}
      >
        <ErrorBoundary
          inline
          message="This view crashed. Reload to recover while keeping navigation available."
          reloadLabel="Reload view"
          testId="route-error-boundary"
          title="View failed to render"
        >
          <Outlet />
        </ErrorBoundary>
      </main>

      <LayoutRightPanel {...rightPanel} />
      <LayoutDialogs {...dialogs} />
      <ToastViewport />
    </div>
  );
}

export const AppLayout = Layout;
