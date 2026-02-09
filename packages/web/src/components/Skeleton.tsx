import clsx from "clsx";
import type { CSSProperties } from "react";
import styles from "./Skeleton.module.css";

export interface SkeletonBlockProps {
  className?: string;
  style?: CSSProperties;
  testId?: string;
}

export interface SkeletonStackProps {
  className?: string;
  lineClassNames: readonly string[];
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

export function SkeletonStack({
  className,
  lineClassNames,
}: SkeletonStackProps) {
  return (
    <div aria-hidden="true" className={className}>
      {lineClassNames.map((lineClassName, index) => (
        <SkeletonBlock
          className={lineClassName}
          key={`${index}-${lineClassName}`}
        />
      ))}
    </div>
  );
}
