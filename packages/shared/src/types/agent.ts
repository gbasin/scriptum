export interface Agent {
  name: string;
  type: "human" | "agent";
  activeSectionIds: string[];
  activeDocumentPath: string | null;
  lastSeenAt: string;
}
