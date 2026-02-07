import type { Document } from "@scriptum/shared";
import { useMemo, useState } from "react";

export interface SearchPanelResult {
  author: string;
  documentId: string;
  documentPath: string;
  id: string;
  lineNumber: number;
  snippet: string;
  tags: string[];
  updatedAt: string;
}

export interface SearchPanelFilters {
  author: string | null;
  dateFrom: string | null;
  dateTo: string | null;
  tag: string | null;
}

export interface SearchShortcutEventLike {
  ctrlKey: boolean;
  key: string;
  metaKey: boolean;
  shiftKey: boolean;
}

export interface HighlightSegment {
  isMatch: boolean;
  text: string;
}

export interface SearchPanelProps {
  onClose?: () => void;
  onResultSelect?: (result: SearchPanelResult) => void;
  results: readonly SearchPanelResult[];
}

const UNKNOWN_AUTHOR = "Unknown";

function normalizeDate(value: string): string | null {
  const parsed = new Date(value);
  if (Number.isNaN(parsed.getTime())) {
    return null;
  }
  return parsed.toISOString().slice(0, 10);
}

export function isSearchPanelShortcut(event: SearchShortcutEventLike): boolean {
  if (!(event.metaKey || event.ctrlKey) || !event.shiftKey) {
    return false;
  }
  return event.key.toLowerCase() === "f";
}

export function buildSearchPanelResults(
  documents: readonly Document[],
): SearchPanelResult[] {
  return documents.map((document) => ({
    author: UNKNOWN_AUTHOR,
    documentId: document.id,
    documentPath: document.path,
    id: `${document.id}:1`,
    lineNumber: 1,
    snippet: `${document.title} (${document.path})`,
    tags: document.tags.slice(),
    updatedAt: document.updatedAt,
  }));
}

export function filterSearchResults(
  results: readonly SearchPanelResult[],
  query: string,
  filters: SearchPanelFilters,
): SearchPanelResult[] {
  const normalizedQuery = query.trim().toLowerCase();

  return results.filter((result) => {
    if (filters.tag && !result.tags.includes(filters.tag)) {
      return false;
    }

    if (filters.author && result.author !== filters.author) {
      return false;
    }

    const updatedDate = normalizeDate(result.updatedAt);
    if (filters.dateFrom && (!updatedDate || updatedDate < filters.dateFrom)) {
      return false;
    }
    if (filters.dateTo && (!updatedDate || updatedDate > filters.dateTo)) {
      return false;
    }

    if (!normalizedQuery) {
      return true;
    }

    const haystacks = [result.documentPath, result.snippet, ...result.tags];
    return haystacks.some((value) =>
      value.toLowerCase().includes(normalizedQuery),
    );
  });
}

export function highlightText(text: string, query: string): HighlightSegment[] {
  const normalizedQuery = query.trim();
  if (!normalizedQuery) {
    return [{ isMatch: false, text }];
  }

  const lowerText = text.toLowerCase();
  const lowerQuery = normalizedQuery.toLowerCase();
  const segments: HighlightSegment[] = [];
  let cursor = 0;

  while (cursor < text.length) {
    const index = lowerText.indexOf(lowerQuery, cursor);
    if (index === -1) {
      segments.push({ isMatch: false, text: text.slice(cursor) });
      break;
    }

    if (index > cursor) {
      segments.push({ isMatch: false, text: text.slice(cursor, index) });
    }

    segments.push({
      isMatch: true,
      text: text.slice(index, index + normalizedQuery.length),
    });
    cursor = index + normalizedQuery.length;
  }

  return segments.filter((segment) => segment.text.length > 0);
}

function uniqueSorted(values: readonly string[]): string[] {
  return Array.from(new Set(values.filter((value) => value.length > 0))).sort(
    (left, right) => left.localeCompare(right),
  );
}

export function SearchPanel({
  onClose,
  onResultSelect,
  results,
}: SearchPanelProps) {
  const [query, setQuery] = useState("");
  const [tagFilter, setTagFilter] = useState<string | null>(null);
  const [authorFilter, setAuthorFilter] = useState<string | null>(null);
  const [dateFrom, setDateFrom] = useState<string | null>(null);
  const [dateTo, setDateTo] = useState<string | null>(null);

  const availableTags = useMemo(
    () => uniqueSorted(results.flatMap((result) => result.tags)),
    [results],
  );
  const availableAuthors = useMemo(
    () => uniqueSorted(results.map((result) => result.author)),
    [results],
  );
  const filteredResults = useMemo(
    () =>
      filterSearchResults(results, query, {
        author: authorFilter,
        dateFrom,
        dateTo,
        tag: tagFilter,
      }),
    [results, query, tagFilter, authorFilter, dateFrom, dateTo],
  );

  return (
    <section aria-label="Search panel" data-testid="search-panel">
      <div
        style={{
          alignItems: "center",
          display: "flex",
          justifyContent: "space-between",
          marginBottom: "0.5rem",
          marginTop: "1rem",
        }}
      >
        <h2 style={{ margin: 0 }}>Search</h2>
        <button
          data-testid="search-panel-close"
          onClick={onClose}
          type="button"
        >
          Close
        </button>
      </div>
      <p style={{ color: "#6b7280", fontSize: "0.8rem", marginTop: 0 }}>
        Shortcut: Cmd/Ctrl+Shift+F
      </p>
      <div style={{ display: "grid", gap: "0.4rem" }}>
        <input
          aria-label="Search query"
          data-testid="search-panel-query"
          onChange={(event) => setQuery(event.target.value)}
          placeholder="Search markdown..."
          type="text"
          value={query}
        />
        <div style={{ display: "flex", gap: "0.4rem" }}>
          <select
            aria-label="Filter by tag"
            data-testid="search-panel-filter-tag"
            onChange={(event) =>
              setTagFilter(event.target.value ? event.target.value : null)
            }
            value={tagFilter ?? ""}
          >
            <option value="">All tags</option>
            {availableTags.map((tag) => (
              <option key={tag} value={tag}>
                {tag}
              </option>
            ))}
          </select>
          <select
            aria-label="Filter by author"
            data-testid="search-panel-filter-author"
            onChange={(event) =>
              setAuthorFilter(event.target.value ? event.target.value : null)
            }
            value={authorFilter ?? ""}
          >
            <option value="">All authors</option>
            {availableAuthors.map((author) => (
              <option key={author} value={author}>
                {author}
              </option>
            ))}
          </select>
        </div>
        <div style={{ display: "flex", gap: "0.4rem" }}>
          <input
            aria-label="Filter from date"
            data-testid="search-panel-filter-date-from"
            onChange={(event) =>
              setDateFrom(event.target.value ? event.target.value : null)
            }
            type="date"
            value={dateFrom ?? ""}
          />
          <input
            aria-label="Filter to date"
            data-testid="search-panel-filter-date-to"
            onChange={(event) =>
              setDateTo(event.target.value ? event.target.value : null)
            }
            type="date"
            value={dateTo ?? ""}
          />
        </div>
      </div>
      {filteredResults.length === 0 ? (
        <p data-testid="search-panel-empty" style={{ marginTop: "0.75rem" }}>
          No matches.
        </p>
      ) : (
        <ul
          data-testid="search-panel-results"
          style={{ listStyle: "none", margin: "0.75rem 0 0", padding: 0 }}
        >
          {filteredResults.map((result) => (
            <li key={result.id} style={{ marginBottom: "0.45rem" }}>
              <button
                data-testid={`search-panel-result-${result.id}`}
                onClick={() => onResultSelect?.(result)}
                style={{
                  background: "#f8fafc",
                  border: "1px solid #d1d5db",
                  borderRadius: "0.4rem",
                  cursor: "pointer",
                  display: "block",
                  padding: "0.45rem 0.55rem",
                  textAlign: "left",
                  width: "100%",
                }}
                type="button"
              >
                <div style={{ fontSize: "0.78rem", fontWeight: 600 }}>
                  {result.documentPath}
                </div>
                <div style={{ color: "#475569", fontSize: "0.72rem" }}>
                  Line {result.lineNumber} · {result.author}
                </div>
                <div style={{ color: "#334155", fontSize: "0.78rem" }}>
                  {highlightText(result.snippet, query).map((segment, index) =>
                    segment.isMatch ? (
                      <mark key={`${result.id}-${index}`}>{segment.text}</mark>
                    ) : (
                      <span key={`${result.id}-${index}`}>{segment.text}</span>
                    ),
                  )}
                </div>
              </button>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
