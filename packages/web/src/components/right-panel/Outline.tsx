import { useEffect, useState } from "react";
import { SkeletonBlock } from "../Skeleton";

const OUTLINE_ACTIVE_OFFSET_PX = 120;

export interface OutlineHeading {
  id: string;
  level: number;
  text: string;
}

export interface OutlineProps {
  editorContainer: HTMLElement | null;
  loading?: boolean;
}

function normalizeHeadingSlug(value: string): string {
  const slug = value
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return slug.length > 0 ? slug : "section";
}

export function collectOutlineHeadings(
  container: HTMLElement | null,
): OutlineHeading[] {
  if (!container) {
    return [];
  }

  const headingElements = Array.from(
    container.querySelectorAll<HTMLElement>("h1,h2,h3,h4,h5,h6"),
  );

  return headingElements.map((heading, index) => {
    const text = heading.textContent?.trim() ?? "";
    if (!heading.id) {
      heading.id = `outline-${normalizeHeadingSlug(text)}-${index + 1}`;
    }

    const headingLevelRaw = Number.parseInt(heading.tagName.slice(1), 10);
    const level = Number.isFinite(headingLevelRaw)
      ? Math.min(6, Math.max(1, headingLevelRaw))
      : 1;

    return {
      id: heading.id,
      level,
      text: text || `Section ${index + 1}`,
    };
  });
}

export function detectActiveOutlineHeadingId(
  container: HTMLElement | null,
): string | null {
  if (!container) {
    return null;
  }

  const headingElements = Array.from(
    container.querySelectorAll<HTMLElement>("h1,h2,h3,h4,h5,h6"),
  );
  if (headingElements.length === 0) {
    return null;
  }

  let activeHeadingId: string | null = null;
  for (const heading of headingElements) {
    if (!heading.id) {
      continue;
    }
    if (heading.getBoundingClientRect().top <= OUTLINE_ACTIVE_OFFSET_PX) {
      activeHeadingId = heading.id;
    }
  }

  if (activeHeadingId) {
    return activeHeadingId;
  }

  return headingElements.find((heading) => heading.id)?.id ?? null;
}

export function Outline({ editorContainer, loading = false }: OutlineProps) {
  const [headings, setHeadings] = useState<OutlineHeading[]>([]);
  const [activeHeadingId, setActiveHeadingId] = useState<string | null>(null);

  useEffect(() => {
    const container = editorContainer;
    if (!container || typeof window === "undefined") {
      setHeadings([]);
      setActiveHeadingId(null);
      return undefined;
    }

    let latestHeadings = collectOutlineHeadings(container);
    setHeadings(latestHeadings);
    setActiveHeadingId(detectActiveOutlineHeadingId(container));

    const refreshOutline = () => {
      latestHeadings = collectOutlineHeadings(container);
      setHeadings(latestHeadings);
      setActiveHeadingId(detectActiveOutlineHeadingId(container));
    };

    const updateActiveHeading = () => {
      if (latestHeadings.length === 0) {
        setActiveHeadingId(null);
        return;
      }
      setActiveHeadingId(detectActiveOutlineHeadingId(container));
    };

    const observer = new MutationObserver(refreshOutline);
    observer.observe(container, {
      characterData: true,
      childList: true,
      subtree: true,
    });

    window.addEventListener("scroll", updateActiveHeading, { passive: true });
    window.addEventListener("resize", updateActiveHeading);

    return () => {
      observer.disconnect();
      window.removeEventListener("scroll", updateActiveHeading);
      window.removeEventListener("resize", updateActiveHeading);
    };
  }, [editorContainer]);

  const handleHeadingClick = (headingId: string) => {
    const container = editorContainer;
    if (!container) {
      return;
    }
    const target = Array.from(
      container.querySelectorAll<HTMLElement>("h1,h2,h3,h4,h5,h6"),
    ).find((heading) => heading.id === headingId);

    if (!target) {
      return;
    }

    target.scrollIntoView({ behavior: "smooth", block: "start" });
    setActiveHeadingId(headingId);
  };

  if (loading) {
    return (
      <div data-testid="outline-loading">
        <div aria-hidden="true" style={{ display: "grid", gap: "0.375rem" }}>
          <SkeletonBlock style={{ height: "0.78rem", width: "78%" }} />
          <SkeletonBlock style={{ height: "0.78rem", width: "62%" }} />
          <SkeletonBlock style={{ height: "0.78rem", width: "72%" }} />
          <SkeletonBlock style={{ height: "0.78rem", width: "52%" }} />
        </div>
      </div>
    );
  }

  if (headings.length === 0) {
    return (
      <p data-testid="outline-empty" style={{ color: "#6b7280", margin: 0 }}>
        No headings in this document.
      </p>
    );
  }

  return (
    <ul
      aria-label="Document heading outline"
      data-testid="outline-list"
      style={{ listStyle: "none", margin: 0, padding: 0 }}
    >
      {headings.map((heading) => (
        <li key={heading.id}>
          <button
            data-active={heading.id === activeHeadingId}
            data-testid={`outline-heading-${heading.id}`}
            onClick={() => handleHeadingClick(heading.id)}
            style={{
              background:
                heading.id === activeHeadingId ? "#e0e7ff" : "transparent",
              border: "none",
              borderRadius: "0.375rem",
              color: "#111827",
              cursor: "pointer",
              display: "block",
              fontSize: "0.875rem",
              marginBottom: "0.25rem",
              overflow: "hidden",
              padding: "0.3rem 0.4rem",
              paddingLeft: `${0.4 + (heading.level - 1) * 0.6}rem`,
              textAlign: "left",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
              width: "100%",
            }}
            title={heading.text}
            type="button"
          >
            {heading.text}
          </button>
        </li>
      ))}
    </ul>
  );
}
