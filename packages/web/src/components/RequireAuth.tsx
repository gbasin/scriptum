// Protected route wrapper — redirects to "/" if unauthenticated.

import { Navigate, useLocation } from "react-router-dom";
import { useAuthStore } from "../store/auth";
import { useRuntimeStore } from "../store/runtime";
import { isFixtureModeEnabled } from "../test/setup";
import styles from "./RequireAuth.module.css";
import { SkeletonBlock } from "./Skeleton";

export function RequireAuth({ children }: { children: React.ReactNode }) {
  const status = useAuthStore((s) => s.status);
  const mode = useRuntimeStore((s) => s.mode);
  const location = useLocation();
  const fixtureModeEnabled = isFixtureModeEnabled();

  if (fixtureModeEnabled || mode === "local") {
    return <>{children}</>;
  }

  if (status === "unknown") {
    return (
      <section
        aria-label="Restoring session"
        className={styles.skeletonPage}
        data-testid="require-auth-skeleton"
      >
        <SkeletonBlock className={styles.titleSkeleton} />
        <SkeletonBlock className={styles.lineSkeleton} />
        <SkeletonBlock className={styles.lineSkeletonShort} />
      </section>
    );
  }

  if (status === "unauthenticated") {
    return <Navigate to="/" replace state={{ from: location.pathname }} />;
  }

  return <>{children}</>;
}
