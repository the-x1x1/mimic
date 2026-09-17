import type { ReactNode } from "react";
import { cx } from "./cx";

export type BadgeTone = "neutral" | "success" | "warning" | "danger" | "info" | "accent";

export function Badge({
  tone = "neutral",
  children,
  className,
  title,
}: {
  tone?: BadgeTone;
  children: ReactNode;
  className?: string;
  title?: string;
}) {
  return (
    <span className={cx("ui-badge", `ui-badge--${tone}`, className)} title={title}>
      {children}
    </span>
  );
}
