import { nameToColor } from "@scriptum/editor";

const LOCAL_TIMELINE_AUTHOR_ID = "local-user";
const LOCAL_TIMELINE_AUTHOR_NAME = "You";

export interface TimelineAuthor {
  color: string;
  id: string;
  name: string;
  type: "agent" | "human";
}

export interface TimelineSnapshotEntry {
  attribution: TimelineAuthor[];
  content: string;
}

export interface AuthorshipSegment {
  author: TimelineAuthor;
  text: string;
}

export interface TimelineDiffSegment {
  kind: "unchanged" | "removed" | "added";
  text: string;
}

export interface TimelinePeer {
  name: string;
  type: "agent" | "human";
}

export interface TimelineAuthorshipSegment {
  author_id: string;
  author_type: "agent" | "human";
  start_offset: number;
  end_offset: number;
}

export const LOCAL_TIMELINE_AUTHOR: TimelineAuthor = {
  color: nameToColor(LOCAL_TIMELINE_AUTHOR_NAME),
  id: LOCAL_TIMELINE_AUTHOR_ID,
  name: LOCAL_TIMELINE_AUTHOR_NAME,
  type: "human",
};

export const UNKNOWN_REMOTE_TIMELINE_AUTHOR: TimelineAuthor = {
  color: nameToColor("Collaborator"),
  id: "remote-collaborator",
  name: "Collaborator",
  type: "human",
};

export function timelineAuthorFromPeer(peer: TimelinePeer): TimelineAuthor {
  return {
    color: nameToColor(peer.name),
    id: `peer:${peer.name.toLowerCase().replace(/[^a-z0-9]+/g, "-") || "remote"}`,
    name: peer.name,
    type: peer.type,
  };
}

export function timelineAuthorFromHistory(
  authorId: string,
  authorType: "agent" | "human",
): TimelineAuthor {
  const normalizedId = authorId.trim() || "unknown-author";
  const normalizedName =
    normalizedId === LOCAL_TIMELINE_AUTHOR_ID
      ? LOCAL_TIMELINE_AUTHOR_NAME
      : normalizedId;
  return {
    color: nameToColor(normalizedName),
    id: normalizedId,
    name: normalizedName,
    type: authorType,
  };
}

export function timelineSnapshotEntryFromAuthorshipSegments(
  content: string,
  segments: TimelineAuthorshipSegment[],
  fallbackAuthor: TimelineAuthor = LOCAL_TIMELINE_AUTHOR,
): TimelineSnapshotEntry {
  const attribution = Array.from(
    { length: content.length },
    () => fallbackAuthor,
  );
  for (const segment of segments) {
    const start = Math.max(
      0,
      Math.min(content.length, Math.floor(segment.start_offset)),
    );
    const end = Math.max(
      start,
      Math.min(content.length, Math.floor(segment.end_offset)),
    );
    if (end <= start) {
      continue;
    }

    const author = timelineAuthorFromHistory(
      segment.author_id,
      segment.author_type,
    );
    for (let index = start; index < end; index += 1) {
      attribution[index] = author;
    }
  }
  return { content, attribution };
}

export function createTimelineSnapshotEntry(
  content: string,
  author: TimelineAuthor,
): TimelineSnapshotEntry {
  return {
    attribution: Array.from({ length: content.length }, () => author),
    content,
  };
}

function normalizedAttributionLength(
  entry: TimelineSnapshotEntry,
): TimelineAuthor[] {
  if (entry.attribution.length === entry.content.length) {
    return entry.attribution;
  }

  return Array.from(
    { length: entry.content.length },
    (_unused, index) => entry.attribution[index] ?? LOCAL_TIMELINE_AUTHOR,
  );
}

export function deriveTimelineSnapshotEntry(
  previousEntry: TimelineSnapshotEntry,
  nextContent: string,
  author: TimelineAuthor,
): TimelineSnapshotEntry {
  if (previousEntry.content === nextContent) {
    return {
      attribution: normalizedAttributionLength(previousEntry).slice(),
      content: previousEntry.content,
    };
  }

  const previousContent = previousEntry.content;
  const previousAttribution = normalizedAttributionLength(previousEntry);
  let prefixLength = 0;

  while (
    prefixLength < previousContent.length &&
    prefixLength < nextContent.length &&
    previousContent[prefixLength] === nextContent[prefixLength]
  ) {
    prefixLength += 1;
  }

  let suffixLength = 0;
  while (
    suffixLength < previousContent.length - prefixLength &&
    suffixLength < nextContent.length - prefixLength &&
    previousContent[previousContent.length - 1 - suffixLength] ===
      nextContent[nextContent.length - 1 - suffixLength]
  ) {
    suffixLength += 1;
  }

  const nextMiddleLength = Math.max(
    0,
    nextContent.length - prefixLength - suffixLength,
  );
  const prefixAttribution = previousAttribution.slice(0, prefixLength);
  const suffixAttribution =
    suffixLength > 0
      ? previousAttribution.slice(previousAttribution.length - suffixLength)
      : [];
  const middleAttribution = Array.from(
    { length: nextMiddleLength },
    () => author,
  );

  return {
    attribution: [
      ...prefixAttribution,
      ...middleAttribution,
      ...suffixAttribution,
    ],
    content: nextContent,
  };
}

export function buildAuthorshipSegments(
  entry: TimelineSnapshotEntry,
): AuthorshipSegment[] {
  const attribution = normalizedAttributionLength(entry);
  const { content } = entry;
  if (content.length === 0) {
    return [];
  }

  const segments: AuthorshipSegment[] = [];
  let currentAuthor = attribution[0] ?? LOCAL_TIMELINE_AUTHOR;
  let currentText = content[0] ?? "";

  for (let index = 1; index < content.length; index += 1) {
    const nextAuthor = attribution[index] ?? LOCAL_TIMELINE_AUTHOR;
    const nextCharacter = content[index] ?? "";

    if (nextAuthor.id === currentAuthor.id) {
      currentText += nextCharacter;
      continue;
    }

    segments.push({ author: currentAuthor, text: currentText });
    currentAuthor = nextAuthor;
    currentText = nextCharacter;
  }

  segments.push({ author: currentAuthor, text: currentText });
  return segments;
}

export function buildTimelineDiffSegments(
  currentContent: string,
  snapshotContent: string,
): TimelineDiffSegment[] {
  if (currentContent.length === 0 && snapshotContent.length === 0) {
    return [];
  }
  if (currentContent === snapshotContent) {
    return [{ kind: "unchanged", text: snapshotContent }];
  }

  let prefixLength = 0;
  while (
    prefixLength < currentContent.length &&
    prefixLength < snapshotContent.length &&
    currentContent[prefixLength] === snapshotContent[prefixLength]
  ) {
    prefixLength += 1;
  }

  let suffixLength = 0;
  while (
    suffixLength < currentContent.length - prefixLength &&
    suffixLength < snapshotContent.length - prefixLength &&
    currentContent[currentContent.length - 1 - suffixLength] ===
      snapshotContent[snapshotContent.length - 1 - suffixLength]
  ) {
    suffixLength += 1;
  }

  const prefix = currentContent.slice(0, prefixLength);
  const removed = currentContent.slice(
    prefixLength,
    currentContent.length - suffixLength,
  );
  const added = snapshotContent.slice(
    prefixLength,
    snapshotContent.length - suffixLength,
  );
  const suffix =
    suffixLength > 0
      ? snapshotContent.slice(snapshotContent.length - suffixLength)
      : "";

  const segments: TimelineDiffSegment[] = [];
  if (prefix.length > 0) {
    segments.push({ kind: "unchanged", text: prefix });
  }
  if (removed.length > 0) {
    segments.push({ kind: "removed", text: removed });
  }
  if (added.length > 0) {
    segments.push({ kind: "added", text: added });
  }
  if (suffix.length > 0) {
    segments.push({ kind: "unchanged", text: suffix });
  }

  return segments;
}

export function authorshipMapFromTimelineEntry(
  entry: TimelineSnapshotEntry,
): Map<{ from: number; to: number }, string> {
  const attribution = normalizedAttributionLength(entry);
  const authorshipMap = new Map<{ from: number; to: number }, string>();
  if (entry.content.length === 0 || attribution.length === 0) {
    return authorshipMap;
  }

  let rangeStart = 0;
  let currentAuthor = attribution[0] ?? LOCAL_TIMELINE_AUTHOR;

  for (let index = 1; index <= entry.content.length; index += 1) {
    const nextAuthor =
      index < attribution.length ? attribution[index] : undefined;
    if (nextAuthor?.id === currentAuthor.id) {
      continue;
    }

    authorshipMap.set({ from: rangeStart, to: index }, currentAuthor.name);
    if (nextAuthor) {
      rangeStart = index;
      currentAuthor = nextAuthor;
    }
  }

  return authorshipMap;
}
