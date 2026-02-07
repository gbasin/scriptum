import type { Document } from "@scriptum/shared";

interface ParsedWikiLink {
  raw: string;
  target: string;
}

interface ParsedWikiLinkParts {
  alias: string | null;
  heading: string | null;
  target: string;
}

export interface RenameBacklinkRewriteResult {
  rewrittenDocuments: Document[];
  updatedDocuments: number;
  updatedLinks: number;
}

export interface IncomingBacklink {
  sourceDocumentId: string;
  sourcePath: string;
  sourceTitle: string;
  snippet: string;
}

function normalizeBacklinkTarget(value: string): string {
  return value.trim().toLowerCase();
}

function baseName(path: string): string {
  const segments = path.split("/").filter(Boolean);
  return segments[segments.length - 1] ?? path;
}

function baseNameWithoutExtension(path: string): string {
  return baseName(path).replace(/\.[^.]+$/, "");
}

function parseWikiLinkParts(rawInner: string): ParsedWikiLinkParts | null {
  const trimmed = rawInner.trim();
  if (!trimmed) {
    return null;
  }

  const [targetWithHeadingRaw, aliasRaw] = trimmed.split("|", 2);
  const [targetRaw, headingRaw] = targetWithHeadingRaw.split("#", 2);
  const target = targetRaw.trim();
  if (!target) {
    return null;
  }

  const heading = headingRaw?.trim() || null;
  const alias = aliasRaw?.trim() || null;
  return { alias, heading, target };
}

function extractWikiLinks(markdown: string): ParsedWikiLink[] {
  const links: ParsedWikiLink[] = [];
  const pattern = /\[\[([^[\]]+)\]\]/g;
  let match: RegExpExecArray | null = pattern.exec(markdown);

  while (match) {
    const parsed = parseWikiLinkParts(match[1] ?? "");
    if (parsed) {
      const normalizedTarget = normalizeBacklinkTarget(parsed.target);
      if (normalizedTarget.length > 0) {
        links.push({
          raw: match[0],
          target: normalizedTarget,
        });
      }
    }
    match = pattern.exec(markdown);
  }

  return links;
}

function targetAliases(
  document: Pick<Document, "path" | "title">,
): Set<string> {
  const aliases = new Set<string>();
  const pathNormalized = normalizeBacklinkTarget(document.path);
  const pathBaseName = normalizeBacklinkTarget(baseName(document.path));
  const pathBaseNameWithoutExtension = normalizeBacklinkTarget(
    baseNameWithoutExtension(document.path),
  );
  const titleNormalized = normalizeBacklinkTarget(document.title);

  if (pathNormalized.length > 0) {
    aliases.add(pathNormalized);
  }
  if (pathBaseName.length > 0) {
    aliases.add(pathBaseName);
  }
  if (pathBaseNameWithoutExtension.length > 0) {
    aliases.add(pathBaseNameWithoutExtension);
  }
  if (titleNormalized.length > 0) {
    aliases.add(titleNormalized);
  }

  return aliases;
}

function replacementTargetForRename(
  originalTarget: string,
  oldDocument: Pick<Document, "path" | "title">,
  nextPath: string,
): string {
  const normalizedOriginalTarget = normalizeBacklinkTarget(originalTarget);
  const normalizedOldPath = normalizeBacklinkTarget(oldDocument.path);
  const normalizedOldBaseName = normalizeBacklinkTarget(
    baseName(oldDocument.path),
  );
  const normalizedOldBaseNameWithoutExtension = normalizeBacklinkTarget(
    baseNameWithoutExtension(oldDocument.path),
  );
  const normalizedOldTitle = normalizeBacklinkTarget(oldDocument.title);

  if (normalizedOriginalTarget === normalizedOldPath) {
    return nextPath;
  }
  if (normalizedOriginalTarget === normalizedOldBaseName) {
    return baseName(nextPath);
  }
  if (
    normalizedOriginalTarget === normalizedOldBaseNameWithoutExtension ||
    normalizedOriginalTarget === normalizedOldTitle
  ) {
    return baseNameWithoutExtension(nextPath);
  }

  return nextPath;
}

export function rewriteWikiReferencesForRename(
  workspaceDocuments: readonly Document[],
  renamedDocument: Pick<Document, "id" | "path" | "title">,
  nextPath: string,
): RenameBacklinkRewriteResult {
  const trimmedNextPath = nextPath.trim();
  if (!trimmedNextPath) {
    return {
      rewrittenDocuments: [],
      updatedDocuments: 0,
      updatedLinks: 0,
    };
  }

  const oldAliases = targetAliases(renamedDocument);
  if (oldAliases.size === 0) {
    return {
      rewrittenDocuments: [],
      updatedDocuments: 0,
      updatedLinks: 0,
    };
  }

  const rewrittenDocuments: Document[] = [];
  let updatedLinks = 0;

  for (const document of workspaceDocuments) {
    if (
      document.id === renamedDocument.id ||
      typeof document.bodyMd !== "string"
    ) {
      continue;
    }

    let documentUpdatedLinks = 0;
    const rewrittenBody = document.bodyMd.replace(
      /\[\[([^[\]]+)\]\]/g,
      (rawMatch, rawInner: string) => {
        const parsed = parseWikiLinkParts(rawInner);
        if (!parsed) {
          return rawMatch;
        }

        const normalizedTarget = normalizeBacklinkTarget(parsed.target);
        if (!oldAliases.has(normalizedTarget)) {
          return rawMatch;
        }

        const replacementTarget = replacementTargetForRename(
          parsed.target,
          renamedDocument,
          trimmedNextPath,
        );
        let replacementInner = replacementTarget;
        if (parsed.heading) {
          replacementInner = `${replacementInner}#${parsed.heading}`;
        }
        if (parsed.alias) {
          replacementInner = `${replacementInner}|${parsed.alias}`;
        }
        documentUpdatedLinks += 1;
        return `[[${replacementInner}]]`;
      },
    );

    if (documentUpdatedLinks > 0) {
      updatedLinks += documentUpdatedLinks;
      rewrittenDocuments.push({
        ...document,
        bodyMd: rewrittenBody,
      });
    }
  }

  return {
    rewrittenDocuments,
    updatedDocuments: rewrittenDocuments.length,
    updatedLinks,
  };
}

export function buildIncomingBacklinks(
  documents: readonly Document[],
  activeDocumentId: string | null,
): IncomingBacklink[] {
  if (!activeDocumentId) {
    return [];
  }

  const activeDocument = documents.find(
    (document) => document.id === activeDocumentId,
  );
  if (!activeDocument) {
    return [];
  }
  const aliases = targetAliases(activeDocument);
  const backlinks: IncomingBacklink[] = [];

  for (const document of documents) {
    if (
      document.id === activeDocument.id ||
      typeof document.bodyMd !== "string"
    ) {
      continue;
    }
    const link = extractWikiLinks(document.bodyMd).find((candidate) =>
      aliases.has(candidate.target),
    );
    if (!link) {
      continue;
    }

    backlinks.push({
      sourceDocumentId: document.id,
      sourcePath: document.path,
      sourceTitle: document.title,
      snippet: link.raw,
    });
  }

  return backlinks.sort((left, right) =>
    left.sourcePath.localeCompare(right.sourcePath),
  );
}
