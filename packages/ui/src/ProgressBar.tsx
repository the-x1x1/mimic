/** Item-based progress, never a fake clock ETA (spec §17.1). */
export function ProgressBar({
  current,
  total,
  label,
}: {
  current: number;
  total: number;
  label?: string;
}) {
  const determinate = total > 0;
  const pct = determinate ? Math.min(100, Math.round((current / total) * 100)) : 0;
  return (
    <div
      className="ui-progress"
      role="progressbar"
      aria-valuemin={0}
      aria-valuemax={determinate ? total : undefined}
      aria-valuenow={determinate ? current : undefined}
      aria-label={label}
    >
      <div
        className={
          determinate ? "ui-progress__bar" : "ui-progress__bar ui-progress__bar--indeterminate"
        }
        style={determinate ? { width: `${pct}%` } : undefined}
      />
      {label ? (
        <div className="ui-progress__label">
          <span>{label}</span>
          <span className="ui-progress__count">
            {determinate ? `${current.toLocaleString()} / ${total.toLocaleString()}` : "working…"}
          </span>
        </div>
      ) : null}
    </div>
  );
}
