import type { ReactNode } from "react";

/** Every page has a real empty state (spec §18): title, plain-language body, one primary action, optional secondary. */
export function EmptyState({
  icon,
  title,
  body,
  primary,
  secondary,
}: {
  icon?: ReactNode;
  title: string;
  body: ReactNode;
  primary?: ReactNode;
  secondary?: ReactNode;
}) {
  return (
    <div className="ui-empty" role="status">
      {icon ? (
        <div className="ui-empty__icon" aria-hidden>
          {icon}
        </div>
      ) : null}
      <h2 className="ui-empty__title">{title}</h2>
      <div className="ui-empty__body">{body}</div>
      {primary || secondary ? (
        <div className="ui-empty__actions">
          {primary}
          {secondary}
        </div>
      ) : null}
    </div>
  );
}
