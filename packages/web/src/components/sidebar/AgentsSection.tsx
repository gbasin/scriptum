import type { PeerPresence } from "../../store/presence";

const ACTIVE_WINDOW_MS = 60_000;

export function activityStatusFromLastSeen(
  lastSeenAt: string,
  nowMs: number = Date.now()
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

export function AgentsSection({ peers, nowMs = Date.now() }: AgentsSectionProps) {
  const agents = peers
    .filter((peer) => peer.type === "agent")
    .slice()
    .sort((left, right) => left.name.localeCompare(right.name));

  return (
    <section aria-label="Agents section" data-testid="sidebar-agents-section">
      <h2 style={{ marginBottom: "0.25rem", marginTop: "1rem" }}>Agents</h2>
      {agents.length === 0 ? (
        <p data-testid="sidebar-agents-empty">No active agents.</p>
      ) : (
        <ul
          data-testid="sidebar-agents-list"
          style={{ listStyle: "none", margin: 0, padding: 0 }}
        >
          {agents.map((agent) => {
            const status = activityStatusFromLastSeen(agent.lastSeenAt, nowMs);
            return (
              <li
                data-testid={`sidebar-agent-${agent.name}`}
                key={agent.name}
                style={{
                  border: "1px solid #d1d5db",
                  borderRadius: "0.5rem",
                  marginTop: "0.5rem",
                  padding: "0.5rem",
                }}
              >
                <div style={{ fontWeight: 600 }}>{agent.name}</div>
                <div data-testid={`sidebar-agent-status-${agent.name}`}>
                  Status: {status}
                </div>
                <div data-testid={`sidebar-agent-document-${agent.name}`}>
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
