import type { ReactNode } from "react";

export function Field({
  label,
  hint,
  htmlFor,
  children,
  error,
}: {
  label: string;
  hint?: ReactNode;
  htmlFor?: string;
  children: ReactNode;
  error?: string;
}) {
  return (
    <div className="ui-field">
      <label className="ui-field__label" htmlFor={htmlFor}>
        {label}
      </label>
      {children}
      {error ? (
        <div className="ui-field__error">{error}</div>
      ) : hint ? (
        <div className="ui-field__hint">{hint}</div>
      ) : null}
    </div>
  );
}
