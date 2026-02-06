// Protected route wrapper — redirects to "/" if unauthenticated.

import { Navigate, useLocation } from "react-router-dom";
import { useAuthStore } from "../store/auth";

export function RequireAuth({ children }: { children: React.ReactNode }) {
  const status = useAuthStore((s) => s.status);
  const location = useLocation();

  if (status === "unknown") {
    return null; // Session restore in progress.
  }

  if (status === "unauthenticated") {
    return <Navigate to="/" replace state={{ from: location.pathname }} />;
  }

  return <>{children}</>;
}
