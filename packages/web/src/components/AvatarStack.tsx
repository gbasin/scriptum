import clsx from "clsx";
import type { CSSProperties } from "react";
import { useMemo } from "react";
import type { PeerPresence } from "../store/presence";
import styles from "./AvatarStack.module.css";

// ── Types ────────────────────────────────────────────────────────────────────

export interface AvatarStackProps {
  /** Peers to display avatars for. */
  peers: PeerPresence[];
  /** Maximum visible avatars before showing "+N" overflow. */
  maxVisible?: number;
  /** Avatar size in pixels. */
  size?: number;
}

// ── Deterministic color from name ────────────────────────────────────────────

const AVATAR_COLORS = [
  "#e74c3c", // red
  "#3498db", // blue
  "#2ecc71", // green
  "#f39c12", // orange
  "#9b59b6", // purple
  "#1abc9c", // teal
  "#e67e22", // dark orange
  "#e91e63", // pink
  "#00bcd4", // cyan
  "#8bc34a", // light green
  "#ff5722", // deep orange
  "#673ab7", // deep purple
];

/** Deterministic color assignment based on name hash. */
export function colorForName(name: string): string {
  let hash = 0;
  for (let i = 0; i < name.length; i++) {
    hash = ((hash << 5) - hash + name.charCodeAt(i)) | 0;
  }
  return AVATAR_COLORS[Math.abs(hash) % AVATAR_COLORS.length];
}

/** Extract initials from a name (up to 2 characters). */
export function initialsForName(name: string): string {
  const parts = name.trim().split(/\s+/);
  if (parts.length >= 2) {
    return (parts[0][0] + parts[parts.length - 1][0]).toUpperCase();
  }
  return name.slice(0, 2).toUpperCase();
}

// ── Avatar component ─────────────────────────────────────────────────────────

function Avatar({
  peer,
  size,
  offset,
}: {
  peer: PeerPresence;
  size: number;
  offset: number;
}) {
  const color = peer.color || colorForName(peer.name);
  const initials = initialsForName(peer.name);
  const isAgent = peer.type === "agent";

  return (
    <div
      aria-label={`${peer.name}${isAgent ? " (agent)" : ""}`}
      className={clsx(styles.avatar, isAgent && styles.avatarAgent)}
      data-testid={`avatar-${peer.name}`}
      role="img"
      style={
        {
          "--avatar-bg-color": color,
          "--avatar-font-size": `${Math.max(size * 0.4, 10)}px`,
          "--avatar-offset": `${offset}px`,
          "--avatar-size": `${size}px`,
          "--avatar-z-index": `${10 - Math.floor(offset / (size * 0.6))}`,
        } as CSSProperties
      }
      title={peer.name}
    >
      {initials}
    </div>
  );
}

function OverflowIndicator({
  count,
  size,
  offset,
}: {
  count: number;
  size: number;
  offset: number;
}) {
  return (
    <div
      aria-label={`${count} more`}
      className={styles.overflowIndicator}
      data-testid="avatar-overflow"
      role="img"
      style={
        {
          "--avatar-font-size": `${Math.max(size * 0.35, 9)}px`,
          "--avatar-offset": `${offset}px`,
          "--avatar-size": `${size}px`,
        } as CSSProperties
      }
      title={`${count} more online`}
    >
      +{count}
    </div>
  );
}

// ── Main component ───────────────────────────────────────────────────────────

export function AvatarStack({
  peers,
  maxVisible = 5,
  size = 32,
}: AvatarStackProps) {
  const sortedPeers = useMemo(
    () => [...peers].sort((a, b) => a.name.localeCompare(b.name)),
    [peers],
  );

  if (sortedPeers.length === 0) {
    return null;
  }

  const visiblePeers = sortedPeers.slice(0, maxVisible);
  const overflowCount = sortedPeers.length - maxVisible;
  const overlap = size * 0.6;
  const totalWidth =
    visiblePeers.length * overlap +
    (overflowCount > 0 ? overlap : 0) +
    size * 0.4;

  return (
    // biome-ignore lint/a11y/useSemanticElements: styled div used intentionally for layout
    <div
      aria-label="Online users"
      className={styles.stack}
      data-testid="avatar-stack"
      role="list"
      style={
        {
          "--stack-height": `${size}px`,
          "--stack-width": `${totalWidth}px`,
        } as CSSProperties
      }
    >
      {visiblePeers.map((peer, index) => (
        <Avatar
          key={peer.name}
          offset={index * overlap}
          peer={peer}
          size={size}
        />
      ))}
      {overflowCount > 0 && (
        <OverflowIndicator
          count={overflowCount}
          offset={visiblePeers.length * overlap}
          size={size}
        />
      )}
    </div>
  );
}
