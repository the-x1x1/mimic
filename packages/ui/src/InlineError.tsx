import type { ReactNode } from "react";

export function InlineError({
  title,
  children,
  action,
}: {
  title?: string;
  children?: ReactNode;
  action?: ReactNode;
}) {
  return (
    <div className="ui-inline-error" role="alert">
      <div>
        {title ? <strong>{title}</strong> : null}
        {children ? <div className="ui-inline-error__body">{children}</div> : null}
      </div>
      {action}
    </div>
  );
}
