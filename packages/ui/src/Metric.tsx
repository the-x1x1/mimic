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
  // A number is set large; a phrase in its place ("not measured yet",
  // "never") is set as words, or it wraps over three lines at a number's size.
  const words = typeof value === "string" && !/^[\d+−-]/.test(value.trim());
  return (
    <div className={`ui-metric ui-metric--${tone ?? "neutral"}`}>
      <div className="ui-metric__label">{label}</div>
      <div className={words ? "ui-metric__value ui-metric__value--words" : "ui-metric__value"}>
        {value}
      </div>
      {hint ? <div className="ui-metric__hint">{hint}</div> : null}
    </div>
  );
}
