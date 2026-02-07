// Protected route wrapper — redirects to "/" if unauthenticated.

import { Navigate, useLocation } from "react-router-dom";
import { useAuthStore } from "../store/auth";
import { isFixtureModeEnabled } from "../test/setup";
import { SkeletonBlock } from "./Skeleton";
import styles from "./RequireAuth.module.css";

export function RequireAuth({ children }: { children: React.ReactNode }) {
  const status = useAuthStore((s) => s.status);
  const location = useLocation();
  const fixtureModeEnabled = isFixtureModeEnabled();

  if (fixtureModeEnabled) {
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
