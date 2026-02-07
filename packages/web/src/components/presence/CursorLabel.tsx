import { nameToColor } from "@scriptum/editor";
import { useEffect, useState } from "react";

const DEFAULT_AUTO_HIDE_MS = 3_000;

export interface CursorPosition {
  x: number;
  y: number;
}

export interface CursorLabelPeer {
  name: string;
  type: "human" | "agent";
  color?: string;
  cursorPosition: CursorPosition | null;
}

export interface CursorLabelProps {
  peer: CursorLabelPeer;
  autoHideMs?: number;
}

function RobotIcon() {
  return (
    <svg
      aria-hidden="true"
      data-testid="cursor-label-agent-icon"
      fill="none"
      height="10"
      viewBox="0 0 16 16"
      width="10"
    >
      <rect
        height="8"
        rx="2"
        stroke="currentColor"
        strokeWidth="1.2"
        width="12"
        x="2"
        y="5"
      />
      <path d="M8 2.5v2" stroke="currentColor" strokeWidth="1.2" />
      <circle cx="5.5" cy="9" fill="currentColor" r="0.9" />
      <circle cx="10.5" cy="9" fill="currentColor" r="0.9" />
    </svg>
  );
}

export function CursorLabel({
  peer,
  autoHideMs = DEFAULT_AUTO_HIDE_MS,
}: CursorLabelProps) {
  const [visible, setVisible] = useState(true);
  const cursorX = peer.cursorPosition?.x ?? null;
  const cursorY = peer.cursorPosition?.y ?? null;

  useEffect(() => {
    if (cursorX === null || cursorY === null) {
      setVisible(false);
      return undefined;
    }

    setVisible(true);
    const timeout = window.setTimeout(() => {
      setVisible(false);
    }, autoHideMs);

    return () => {
      window.clearTimeout(timeout);
    };
  }, [autoHideMs, cursorX, cursorY]);

  if (!peer.cursorPosition || !visible) {
    return null;
  }

  const color = peer.color || nameToColor(peer.name);

  return (
    <div
      aria-label={`${peer.name} cursor label`}
      data-testid="cursor-label"
      style={{
        alignItems: "center",
        backgroundColor: color,
        border: "1px solid rgba(0, 0, 0, 0.22)",
        borderRadius: "6px",
        boxShadow: "0 2px 8px rgba(0, 0, 0, 0.22)",
        color: "#ffffff",
        display: "inline-flex",
        fontSize: "12px",
        fontWeight: 600,
        gap: "6px",
        left: `${peer.cursorPosition.x}px`,
        lineHeight: 1,
        maxWidth: "240px",
        padding: "4px 8px",
        pointerEvents: "none",
        position: "absolute",
        top: `${peer.cursorPosition.y}px`,
        transform: "translate(-50%, calc(-100% - 10px))",
        whiteSpace: "nowrap",
        zIndex: 30,
      }}
    >
      {peer.type === "agent" ? <RobotIcon /> : null}
      <span data-testid="cursor-label-name">{peer.name}</span>
    </div>
  );
}
