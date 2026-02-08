import { Link } from "react-router-dom";
import styles from "./not-found.module.css";

export function NotFoundRoute() {
  return (
    <main className={styles.root} data-testid="not-found-route">
      <h1 className={styles.title}>Page not found</h1>
      <p className={styles.description}>
        The page you requested does not exist.
      </p>
      <Link className={styles.homeLink} data-testid="not-found-home-link" to="/">
        Back to home
      </Link>
    </main>
  );
}
