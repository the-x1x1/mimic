import { Button, ProgressBar } from "@mimic/ui";
import { JOB_LABELS } from "@mimic/contracts";
import { useCancelJob, useJobs } from "@/hooks/useJobs";

export function JobTray() {
  const jobs = useJobs(true);
  const cancel = useCancelJob();
  const active = jobs.data ?? [];
  // Nothing running says nothing: the bar is for what is happening and where
  // the writing is done, and "No background work" was one more thing to read.
  if (active.length === 0) return null;
  return (
    <div className="jobtray">
      {active.slice(0, 3).map((j) => (
        <div key={j.id} className="jobtray__job">
          <div className="jobtray__head">
            <span>{JOB_LABELS[j.type] ?? j.type}</span>
            <Button
              size="sm"
              variant="ghost"
              onClick={() => cancel.mutate(j.id)}
              disabled={j.status !== "running" && j.status !== "queued"}
            >
              Stop
            </Button>
          </div>
          <ProgressBar
            current={j.progressCurrent}
            total={j.progressTotal}
            label={j.status === "queued" ? "queued" : (j.phase ?? "working")}
          />
        </div>
      ))}
      {active.length > 3 ? (
        <div className="muted small">+{active.length - 3} more queued</div>
      ) : null}
    </div>
  );
}
