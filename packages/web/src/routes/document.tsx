import { useParams } from "react-router-dom";

export function DocumentRoute() {
  const { workspaceId, documentId } = useParams();

  return (
    <section>
      Document: {workspaceId ?? "unknown"}/{documentId ?? "unknown"}
    </section>
  );
}
