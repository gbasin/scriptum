import clsx from "clsx";
import { useCallback, useEffect, useState } from "react";
import { getAccessToken } from "../../lib/auth";
import controls from "../../styles/Controls.module.css";
import { SkeletonBlock } from "../Skeleton";
import styles from "./Backlinks.module.css";

const RELAY_URL =
  import.meta.env.VITE_SCRIPTUM_RELAY_URL ?? "http://localhost:8080";

export interface BacklinkEntry {
  docId: string;
  path: string;
  title: string;
  linkText: string;
  snippet: string;
}

export interface BacklinksProps {
  workspaceId: string;
  documentId: string;
  backlinks?: BacklinkEntry[];
  loading?: boolean;
  error?: string | null;
  onRetry?: () => void;
  refreshToken?: string | number;
  fetchBacklinks?: (
    workspaceId: string,
    documentId: string,
  ) => Promise<BacklinkEntry[]>;
  onBacklinkSelect?: (sourceDocumentId: string) => void;
}

interface RelayBacklinksResponse {
  backlinks?: unknown[];
  context?: {
    backlinks?: unknown[];
  };
}

function asRecord(value: unknown): Record<string, unknown> | null {
  if (!value || typeof value !== "object") {
    return null;
  }
  return value as Record<string, unknown>;
}

function readString(
  record: Record<string, unknown> | null,
  keys: readonly string[],
): string | null {
  if (!record) {
    return null;
  }
  for (const key of keys) {
    const value = record[key];
    if (typeof value === "string" && value.trim().length > 0) {
      return value;
    }
  }
  return null;
}

function normalizeBacklink(value: unknown): BacklinkEntry | null {
  const record = asRecord(value);
  const docId = readString(record, ["docId", "doc_id", "id"]);
  const path = readString(record, ["path", "source_path"]);
  const title = readString(record, ["title", "source_title"]) ?? path;
  const snippet = readString(record, ["snippet", "context"]) ?? "";
  const linkTextRaw = readString(record, ["linkText", "link_text", "link"]);

  if (!docId || !path || !title) {
    return null;
  }

  const linkText =
    linkTextRaw?.startsWith("[[") && linkTextRaw.endsWith("]]")
      ? linkTextRaw
      : `[[${linkTextRaw ?? title}]]`;

  return {
    docId,
    path,
    title,
    linkText,
    snippet,
  };
}

export function normalizeBacklinksResponse(payload: unknown): BacklinkEntry[] {
  const record = asRecord(payload as RelayBacklinksResponse);
  const context = asRecord(record?.context);
  const backlinksRaw: unknown[] = Array.isArray(record?.backlinks)
    ? record.backlinks
    : Array.isArray(context?.backlinks)
      ? context.backlinks
      : [];

  const backlinks: BacklinkEntry[] = [];
  const seenDocIds = new Set<string>();
  for (const value of backlinksRaw) {
    const backlink = normalizeBacklink(value);
    if (!backlink || seenDocIds.has(backlink.docId)) {
      continue;
    }
    seenDocIds.add(backlink.docId);
    backlinks.push(backlink);
  }
  return backlinks;
}

export async function fetchBacklinksFromRelay(
  workspaceId: string,
  documentId: string,
): Promise<BacklinkEntry[]> {
  const token = await getAccessToken();
  const url = new URL(
    `/v1/workspaces/${encodeURIComponent(workspaceId)}/documents/${encodeURIComponent(
      documentId,
    )}`,
    RELAY_URL,
  );
  url.searchParams.set("include_backlinks", "true");

  const headers = new Headers();
  if (token) {
    headers.set("Authorization", `Bearer ${token}`);
  }

  const response = await fetch(url.toString(), {
    headers,
    method: "GET",
  });
  if (!response.ok) {
    throw new Error(`Backlinks request failed (${response.status})`);
  }

  const payload = (await response.json()) as RelayBacklinksResponse;
  return normalizeBacklinksResponse(payload);
}

export function Backlinks({
  workspaceId,
  documentId,
  backlinks: controlledBacklinks,
  loading: controlledLoading,
  error: controlledError,
  onRetry,
  refreshToken,
  fetchBacklinks = fetchBacklinksFromRelay,
  onBacklinkSelect,
}: BacklinksProps) {
  const controlledMode =
    controlledBacklinks !== undefined ||
    controlledLoading !== undefined ||
    controlledError !== undefined;
  const [backlinks, setBacklinks] = useState<BacklinkEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const loadBacklinks = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const next = await fetchBacklinks(workspaceId, documentId);
      setBacklinks(next);
    } catch {
      setError("Failed to load backlinks.");
    } finally {
      setLoading(false);
    }
  }, [documentId, fetchBacklinks, workspaceId]);

  useEffect(() => {
    if (controlledMode) {
      return;
    }
    void loadBacklinks();
  }, [controlledMode, loadBacklinks, refreshToken]);

  const activeBacklinks = controlledBacklinks ?? backlinks;
  const activeLoading = controlledLoading ?? loading;
  const activeError = controlledError ?? error;
  const handleRetry = onRetry ?? (() => void loadBacklinks());

  return (
    <section
      aria-label="Incoming backlinks panel"
      className={styles.root}
      data-testid="backlinks-panel"
    >
      <h3 className={styles.heading}>Backlinks</h3>

      {activeLoading ? (
        <div data-testid="backlinks-loading">
          <div aria-hidden="true" className={styles.loadingList}>
            <SkeletonBlock className={clsx(styles.loadingLine, styles.loading74)} />
            <SkeletonBlock className={clsx(styles.loadingLine, styles.loading59)} />
            <SkeletonBlock className={clsx(styles.loadingLine, styles.loading68)} />
          </div>
        </div>
      ) : null}

      {!activeLoading && activeError ? (
        <div data-testid="backlinks-error">
          <p className={styles.errorMessage}>{activeError}</p>
          <button
            className={clsx(
              controls.buttonBase,
              controls.buttonSecondary,
              styles.retryButton,
            )}
            data-testid="backlinks-retry"
            onClick={handleRetry}
            type="button"
          >
            Retry
          </button>
        </div>
      ) : null}

      {!activeLoading && !activeError && activeBacklinks.length === 0 ? (
        <p className={styles.emptyState} data-testid="backlinks-empty">
          No documents link to this page.
        </p>
      ) : null}

      {!activeLoading && !activeError && activeBacklinks.length > 0 ? (
        <ul
          aria-label="Incoming wiki links"
          className={styles.backlinksList}
          data-testid="backlinks-list"
        >
          {activeBacklinks.map((backlink) => (
            <li className={styles.backlinkListItem} key={backlink.docId}>
              <button
                className={styles.backlinkTitleButton}
                data-testid={`backlink-item-${backlink.docId}`}
                onClick={() => onBacklinkSelect?.(backlink.docId)}
                type="button"
              >
                {backlink.title}
              </button>
              <p
                data-testid={`backlink-path-${backlink.docId}`}
                className={styles.backlinkPath}
              >
                {backlink.path}
              </p>
              <p
                data-testid={`backlink-link-${backlink.docId}`}
                className={styles.backlinkLinkText}
              >
                {backlink.linkText}
              </p>
              <p
                data-testid={`backlink-snippet-${backlink.docId}`}
                className={styles.backlinkSnippet}
              >
                {backlink.snippet}
              </p>
            </li>
          ))}
        </ul>
      ) : null}
    </section>
  );
}
