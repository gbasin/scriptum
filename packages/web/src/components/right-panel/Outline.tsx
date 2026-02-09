import clsx from "clsx";
import { useEffect, useState } from "react";
import { SkeletonStack } from "../Skeleton";
import styles from "./Outline.module.css";

const OUTLINE_ACTIVE_OFFSET_PX = 120;
const LEVEL_CLASSNAME: Record<number, string> = {
  1: styles.level1,
  2: styles.level2,
  3: styles.level3,
  4: styles.level4,
  5: styles.level5,
  6: styles.level6,
};
const OUTLINE_LOADING_LINE_CLASSNAMES = [
  clsx(styles.loadingLine, styles.loading78),
  clsx(styles.loadingLine, styles.loading62),
  clsx(styles.loadingLine, styles.loading72),
  clsx(styles.loadingLine, styles.loading52),
];

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
        <SkeletonStack
          className={styles.loadingList}
          lineClassNames={OUTLINE_LOADING_LINE_CLASSNAMES}
        />
      </div>
    );
  }

  if (headings.length === 0) {
    return (
      <p className={styles.emptyState} data-testid="outline-empty">
        No headings in this document.
      </p>
    );
  }

  return (
    <ul
      aria-label="Document heading outline"
      className={styles.list}
      data-testid="outline-list"
    >
      {headings.map((heading) => (
        <li key={heading.id}>
          <button
            className={clsx(
              styles.headingButton,
              LEVEL_CLASSNAME[heading.level] ?? styles.level1,
              heading.id === activeHeadingId && styles.headingButtonActive,
            )}
            data-active={heading.id === activeHeadingId}
            data-testid={`outline-heading-${heading.id}`}
            onClick={() => handleHeadingClick(heading.id)}
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
