import type { Document } from "@scriptum/shared";
import clsx from "clsx";
import { useMemo, useState } from "react";
import controls from "../../styles/Controls.module.css";
import { SkeletonBlock } from "../Skeleton";
import styles from "./SearchPanel.module.css";

export interface SearchPanelResult {
  author: string;
  documentId: string;
  documentPath: string;
  documentTitle: string;
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
  loading?: boolean;
  onClose?: () => void;
  onResultSelect?: (result: SearchPanelResult) => void;
  results: readonly SearchPanelResult[];
}

const UNKNOWN_AUTHOR = "Unknown";
const MAX_SNIPPET_LENGTH = 160;

function normalizeSnippetText(value: string): string {
  const trimmed = value.replace(/\s+/g, " ").trim();
  if (trimmed.length <= MAX_SNIPPET_LENGTH) {
    return trimmed;
  }
  return `${trimmed.slice(0, MAX_SNIPPET_LENGTH - 1)}…`;
}

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
  const results: SearchPanelResult[] = [];

  for (const document of documents) {
    const bodyMarkdown =
      typeof document.bodyMd === "string" ? document.bodyMd : "";
    const lines = bodyMarkdown.split(/\r?\n/);
    let appendedLineMatch = false;

    for (let index = 0; index < lines.length; index += 1) {
      const lineNumber = index + 1;
      const snippet = normalizeSnippetText(lines[index] ?? "");
      if (!snippet) {
        continue;
      }

      appendedLineMatch = true;
      results.push({
        author: UNKNOWN_AUTHOR,
        documentId: document.id,
        documentPath: document.path,
        documentTitle: document.title,
        id: `${document.id}:${lineNumber}`,
        lineNumber,
        snippet,
        tags: document.tags.slice(),
        updatedAt: document.updatedAt,
      });
    }

    if (appendedLineMatch) {
      continue;
    }

    results.push({
      author: UNKNOWN_AUTHOR,
      documentId: document.id,
      documentPath: document.path,
      documentTitle: document.title,
      id: `${document.id}:1`,
      lineNumber: 1,
      snippet: `${document.title} (${document.path})`,
      tags: document.tags.slice(),
      updatedAt: document.updatedAt,
    });
  }

  return results;
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

    const haystacks = [
      result.documentPath,
      result.documentTitle,
      result.snippet,
      ...result.tags,
    ];
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
  loading = false,
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
      <div className={styles.header}>
        <h2 className={styles.heading}>Search</h2>
        <button
          className={clsx(controls.buttonBase, controls.buttonSecondary)}
          data-testid="search-panel-close"
          onClick={onClose}
          type="button"
        >
          Close
        </button>
      </div>
      <p className={styles.shortcutHint}>Shortcut: Cmd/Ctrl+Shift+F</p>
      <div className={styles.filtersGrid}>
        <input
          aria-label="Search query"
          className={controls.textInput}
          data-testid="search-panel-query"
          onChange={(event) => setQuery(event.target.value)}
          placeholder="Search markdown..."
          type="text"
          value={query}
        />
        <div className={styles.filterRow}>
          <select
            aria-label="Filter by tag"
            className={controls.selectInput}
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
            className={controls.selectInput}
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
        <div className={styles.filterRow}>
          <input
            aria-label="Filter from date"
            className={controls.textInput}
            data-testid="search-panel-filter-date-from"
            onChange={(event) =>
              setDateFrom(event.target.value ? event.target.value : null)
            }
            type="date"
            value={dateFrom ?? ""}
          />
          <input
            aria-label="Filter to date"
            className={controls.textInput}
            data-testid="search-panel-filter-date-to"
            onChange={(event) =>
              setDateTo(event.target.value ? event.target.value : null)
            }
            type="date"
            value={dateTo ?? ""}
          />
        </div>
      </div>
      {loading ? (
        <ul
          aria-hidden="true"
          className={styles.loadingList}
          data-testid="search-panel-loading"
        >
          {[0, 1, 2, 3].map((index) => (
            <li className={styles.resultItem} key={`skeleton-${index}`}>
              <SkeletonBlock className={styles.loadingCard} />
            </li>
          ))}
        </ul>
      ) : filteredResults.length === 0 ? (
        <p className={styles.emptyState} data-testid="search-panel-empty">
          No matches.
        </p>
      ) : (
        <ul className={styles.resultsList} data-testid="search-panel-results">
          {filteredResults.map((result) => (
            <li className={styles.resultItem} key={result.id}>
              <button
                className={styles.resultButton}
                data-testid={`search-panel-result-${result.id}`}
                onClick={() => onResultSelect?.(result)}
                type="button"
              >
                <div className={styles.resultPath}>{result.documentPath}</div>
                <div className={styles.resultMeta}>
                  Line {result.lineNumber} · {result.author}
                </div>
                <div className={styles.resultSnippet}>
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
