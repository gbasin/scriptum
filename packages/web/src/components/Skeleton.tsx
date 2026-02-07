import clsx from "clsx";
import type { CSSProperties } from "react";
import styles from "./Skeleton.module.css";

export interface SkeletonBlockProps {
  className?: string;
  style?: CSSProperties;
  testId?: string;
}

export function SkeletonBlock({
  className,
  style,
  testId,
}: SkeletonBlockProps) {
  return (
    <span
      aria-hidden="true"
      className={clsx(styles.block, className)}
      data-testid={testId}
      style={style}
    />
  );
}
