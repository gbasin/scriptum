import type { Document } from "@scriptum/shared";

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
      <h2 style={{ marginBottom: "0.25rem", marginTop: "1rem" }}>Tags</h2>
      {tags.length === 0 ? (
        <p data-testid="sidebar-tags-empty">No tags available.</p>
      ) : (
        <ul
          data-testid="sidebar-tags-list"
          style={{
            display: "flex",
            flexWrap: "wrap",
            gap: "0.35rem",
            listStyle: "none",
            margin: 0,
            padding: 0,
          }}
        >
          {tags.map((tag) => {
            const isActive = activeTag === tag;
            return (
              <li key={tag}>
                <button
                  aria-pressed={isActive}
                  data-testid={`sidebar-tag-chip-${tagChipTestId(tag)}`}
                  onClick={() =>
                    onTagSelect?.(toggleTagSelection(activeTag, tag))
                  }
                  style={{
                    background: isActive ? "#dbeafe" : "#f3f4f6",
                    border: "1px solid #d1d5db",
                    borderRadius: "999px",
                    color: isActive ? "#1d4ed8" : "#111827",
                    cursor: "pointer",
                    fontSize: "0.75rem",
                    padding: "0.125rem 0.5rem",
                  }}
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
