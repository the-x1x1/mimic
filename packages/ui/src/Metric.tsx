import type { ReactNode } from "react";

export function Metric({
  label,
  value,
  hint,
  tone,
}: {
  label: string;
  value: ReactNode;
  hint?: ReactNode;
  tone?: "neutral" | "good" | "warn" | "bad";
}) {
  return (
    <div className={`ui-metric ui-metric--${tone ?? "neutral"}`}>
      <div className="ui-metric__label">{label}</div>
      <div className="ui-metric__value">{value}</div>
      {hint ? <div className="ui-metric__hint">{hint}</div> : null}
    </div>
  );
}
