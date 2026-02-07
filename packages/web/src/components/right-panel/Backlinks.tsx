import { useCallback, useEffect, useState } from "react";
import { getAccessToken } from "../../lib/auth";

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
    linkTextRaw && linkTextRaw.startsWith("[[") && linkTextRaw.endsWith("]]")
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
  refreshToken,
  fetchBacklinks = fetchBacklinksFromRelay,
  onBacklinkSelect,
}: BacklinksProps) {
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
    void loadBacklinks();
  }, [loadBacklinks, refreshToken]);

  return (
    <section
      aria-label="Incoming backlinks panel"
      data-testid="backlinks-panel"
    >
      <h3 style={{ margin: "0 0 0.5rem" }}>Backlinks</h3>

      {loading ? (
        <p
          data-testid="backlinks-loading"
          style={{ color: "#6b7280", margin: 0 }}
        >
          Loading backlinks...
        </p>
      ) : null}

      {!loading && error ? (
        <div data-testid="backlinks-error">
          <p style={{ color: "#b91c1c", margin: 0 }}>{error}</p>
          <button
            data-testid="backlinks-retry"
            onClick={() => void loadBacklinks()}
            style={{ marginTop: "0.4rem" }}
            type="button"
          >
            Retry
          </button>
        </div>
      ) : null}

      {!loading && !error && backlinks.length === 0 ? (
        <p
          data-testid="backlinks-empty"
          style={{ color: "#6b7280", margin: 0 }}
        >
          No documents link to this page.
        </p>
      ) : null}

      {!loading && !error && backlinks.length > 0 ? (
        <ul
          aria-label="Incoming wiki links"
          data-testid="backlinks-list"
          style={{ listStyle: "none", margin: 0, padding: 0 }}
        >
          {backlinks.map((backlink) => (
            <li key={backlink.docId} style={{ marginBottom: "0.75rem" }}>
              <button
                data-testid={`backlink-item-${backlink.docId}`}
                onClick={() => onBacklinkSelect?.(backlink.docId)}
                style={{
                  background: "transparent",
                  border: "none",
                  color: "#1d4ed8",
                  cursor: "pointer",
                  fontSize: "0.875rem",
                  fontWeight: 600,
                  padding: 0,
                  textAlign: "left",
                }}
                type="button"
              >
                {backlink.title}
              </button>
              <p
                data-testid={`backlink-path-${backlink.docId}`}
                style={{
                  color: "#4b5563",
                  fontSize: "0.78rem",
                  margin: "0.15rem 0 0",
                }}
              >
                {backlink.path}
              </p>
              <p
                data-testid={`backlink-link-${backlink.docId}`}
                style={{
                  color: "#111827",
                  fontSize: "0.78rem",
                  margin: "0.15rem 0 0",
                }}
              >
                {backlink.linkText}
              </p>
              <p
                data-testid={`backlink-snippet-${backlink.docId}`}
                style={{
                  color: "#6b7280",
                  fontSize: "0.8rem",
                  margin: "0.15rem 0 0",
                }}
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
