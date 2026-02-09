import { useEffect, useState } from "react";
import type { PeerPresence } from "../../store/presence";
import styles from "./AgentsSection.module.css";

const ACTIVE_WINDOW_MS = 60_000;
const ACTIVITY_REFRESH_INTERVAL_MS = 15_000;

export function activityStatusFromLastSeen(
  lastSeenAt: string,
  nowMs: number = Date.now(),
): "active" | "idle" {
  const timestamp = Date.parse(lastSeenAt);
  if (!Number.isFinite(timestamp)) {
    return "idle";
  }
  return nowMs - timestamp <= ACTIVE_WINDOW_MS ? "active" : "idle";
}

interface AgentsSectionProps {
  peers: PeerPresence[];
  nowMs?: number;
}

export function AgentsSection({ peers, nowMs }: AgentsSectionProps) {
  const [clockNowMs, setClockNowMs] = useState(() => Date.now());

  useEffect(() => {
    if (typeof nowMs === "number") {
      return undefined;
    }
    const intervalId = window.setInterval(() => {
      setClockNowMs(Date.now());
    }, ACTIVITY_REFRESH_INTERVAL_MS);
    return () => {
      window.clearInterval(intervalId);
    };
  }, [nowMs]);

  const resolvedNowMs = nowMs ?? clockNowMs;
  const agents = peers
    .filter((peer) => peer.type === "agent")
    .slice()
    .sort((left, right) => left.name.localeCompare(right.name));

  return (
    <section aria-label="Agents section" data-testid="sidebar-agents-section">
      <h2 className={styles.heading}>Agents</h2>
      {agents.length === 0 ? (
        <p className={styles.emptyState} data-testid="sidebar-agents-empty">
          No active agents.
        </p>
      ) : (
        <ul className={styles.agentsList} data-testid="sidebar-agents-list">
          {agents.map((agent) => {
            const status = activityStatusFromLastSeen(
              agent.lastSeenAt,
              resolvedNowMs,
            );
            return (
              <li
                className={styles.agentCard}
                data-testid={`sidebar-agent-${agent.name}`}
                key={agent.name}
              >
                <div className={styles.agentName}>{agent.name}</div>
                <div
                  className={styles.agentMeta}
                  data-testid={`sidebar-agent-status-${agent.name}`}
                >
                  Status: {status}
                </div>
                <div
                  className={styles.agentMeta}
                  data-testid={`sidebar-agent-document-${agent.name}`}
                >
                  Editing: {agent.activeDocumentPath ?? "No document"}
                </div>
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}
