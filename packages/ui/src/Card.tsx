import type { HTMLAttributes, ReactNode } from "react";
import { cx } from "./cx";

export function Card({
  title,
  actions,
  children,
  className,
  ...rest
}: {
  title?: ReactNode;
  actions?: ReactNode;
  children: ReactNode;
} & HTMLAttributes<HTMLDivElement>) {
  return (
    <section className={cx("ui-card", className)} {...rest}>
      {title || actions ? (
        <header className="ui-card__header">
          {title ? <h3 className="ui-card__title">{title}</h3> : <span />}
          {actions ? <div className="ui-card__actions">{actions}</div> : null}
        </header>
      ) : null}
      <div className="ui-card__body">{children}</div>
    </section>
  );
}
