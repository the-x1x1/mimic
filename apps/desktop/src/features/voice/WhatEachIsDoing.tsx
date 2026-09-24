import { Link } from "react-router-dom";
import { Button, Card } from "@mimic/ui";
import { JOB_KINDS } from "@mimic/contracts";
import { useSituationFiling, useStartReadingSituations } from "@/hooks/useVoice";
import { useJobs } from "@/hooks/useJobs";
import { useProviderHealth } from "@/hooks/useCompose";
import { countOf } from "@/lib/format";

/**
 * How the user's messages came to be filed by what they are doing — saying
 * no, setting a time, thanking — which the "When you…" layers are measured
 * over: by the rules, by a model on this computer, or by the user. With a
 * model on this computer, it can be asked to read them; without one, nothing
 * is sent anywhere to do it.
 */
export function WhatEachIsDoing() {
  const filing = useSituationFiling();
  const start = useStartReadingSituations();
  const jobs = useJobs(true);
  const f = filing.data;
  // A model on this computer is set up on every install; whether one is
  // running is another matter, so it is asked before it is offered.
  const health = useProviderHealth(f?.localProvider ?? undefined);
  if (!f) return null;
  const reading = (jobs.data ?? []).some(
    (j) => j.type === JOB_KINDS.readSituations && (j.status === "queued" || j.status === "running"),
  );
  const parts = [
    f.byRules > 0 ? `${countOf(f.byRules, "message")} left to the rules` : null,
    f.byModel > 0 ? `${countOf(f.byModel, "message")} read by the model on this computer` : null,
    f.byYou > 0 ? `${countOf(f.byYou, "message")} you said yourself` : null,
  ].filter((p): p is string => p !== null);

  return (
    <Card title="What each message is doing">
      <p>
        The &ldquo;When you&hellip;&rdquo; layers below are measured over your messages filed by
        what they were doing.{" "}
        {parts.length > 0 ? `So far: ${parts.join("; ")}.` : "Nothing is filed yet."}
      </p>
      <p className="muted small">
        To say what one message was doing, open its conversation on the home screen or under People
        and choose &ldquo;What was this doing?&rdquo; or &ldquo;Change&rdquo; under it. What you say
        stands over the rules and the model.
      </p>
      {f.localModel && health.data?.reachable === false ? (
        <p className="muted small">
          A model reads them better than the rules do, and {f.localModel} would do it on this
          computer, but it isn&rsquo;t answering ({health.data.error ?? "no answer"}). Start it, or
          see <Link to="/settings">Settings</Link>.
        </p>
      ) : f.localModel ? (
        <div className="stack gap-1">
          <div className="row gap-2">
            <Button
              variant="secondary"
              disabled={
                start.isPending || reading || f.byRules === 0 || health.data?.reachable !== true
              }
              onClick={() => start.mutate()}
            >
              {reading ? "Reading them…" : `Have ${f.localModel} read them`}
            </Button>
          </div>
          <p className="muted small">
            {f.localModel} runs on this computer, so reading them sends nothing anywhere. It reads
            the ones nobody else has decided about, up to 400 at a time, the most recent first, and
            what it says replaces what the rules said.
          </p>
        </div>
      ) : (
        <p className="muted small">
          A model reads them better than the rules do, but reading them sends every one to it, so I
          only do that with a model on this computer. You can set one up in{" "}
          <Link to="/settings">Settings</Link>.
        </p>
      )}
    </Card>
  );
}
