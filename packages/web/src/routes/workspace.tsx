import { useParams } from "react-router-dom";

export function WorkspaceRoute() {
  const { workspaceId } = useParams();

  return <section>Workspace: {workspaceId ?? "unknown"}</section>;
}
