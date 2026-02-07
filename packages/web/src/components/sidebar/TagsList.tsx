import type { Document } from "@scriptum/shared";
import clsx from "clsx";
import styles from "./TagsList.module.css";

export interface TagsListProps {
  activeTag: string | null;
  onTagSelect?: (tag: string | null) => void;
  tags: readonly string[];
}

function normalizeTag(tag: string): string {
  return tag.trim();
}

export function tagChipTestId(tag: string): string {
  const normalized = normalizeTag(tag)
    .toLowerCase()
    .replaceAll(/[^a-z0-9]+/g, "-")
    .replaceAll(/^-+|-+$/g, "");
  return normalized.length > 0 ? normalized : "tag";
}

export function collectWorkspaceTags(documents: readonly Document[]): string[] {
  const tags = new Set<string>();

  for (const document of documents) {
    for (const tag of document.tags) {
      const normalized = normalizeTag(tag);
      if (normalized.length === 0) {
        continue;
      }
      tags.add(normalized);
    }
  }

  return Array.from(tags).sort((left, right) => left.localeCompare(right));
}

export function filterDocumentsByTag(
  documents: readonly Document[],
  activeTag: string | null,
): Document[] {
  if (!activeTag) {
    return documents.slice();
  }

  return documents.filter((document) =>
    document.tags.some((tag) => normalizeTag(tag) === activeTag),
  );
}

export function toggleTagSelection(
  activeTag: string | null,
  clickedTag: string,
): string | null {
  return activeTag === clickedTag ? null : clickedTag;
}

export function TagsList({ activeTag, onTagSelect, tags }: TagsListProps) {
  return (
    <section aria-label="Tags section" data-testid="sidebar-tags-section">
      <h2 className={styles.heading}>Tags</h2>
      {tags.length === 0 ? (
        <p className={styles.emptyState} data-testid="sidebar-tags-empty">
          No tags available.
        </p>
      ) : (
        <ul className={styles.tagsList} data-testid="sidebar-tags-list">
          {tags.map((tag) => {
            const isActive = activeTag === tag;
            return (
              <li key={tag}>
                <button
                  aria-pressed={isActive}
                  className={clsx(
                    styles.tagChip,
                    isActive && styles.tagChipActive,
                  )}
                  data-testid={`sidebar-tag-chip-${tagChipTestId(tag)}`}
                  onClick={() =>
                    onTagSelect?.(toggleTagSelection(activeTag, tag))
                  }
                  type="button"
                >
                  #{tag}
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}
