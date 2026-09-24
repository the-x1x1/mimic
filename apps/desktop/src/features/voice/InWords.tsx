import { Button, Card } from "@mimic/ui";
import { JOB_KINDS } from "@mimic/contracts";
import { useProviderHealth, useProviderState } from "@/hooks/useCompose";
import { useJobs } from "@/hooks/useJobs";
import { useStartDescribing } from "@/hooks/useVoice";

/**
 * Putting how the user writes into words: the chosen provider reads each
 * measured layer's numbers — only the numbers, and which of Mimic's own
 * greetings and sign-offs the user opens and closes with — and writes two or
 * three sentences, kept beside them as its reading. Only when asked, and
 * saying where the numbers go.
 */
export function InWords({ measured }: { measured: number }) {
  const providers = useProviderState();
  const start = useStartDescribing();
  const jobs = useJobs(true);
  const state = providers.data;
  const active = state?.providers.find((p) => p.id === state.active) ?? null;
  const health = useProviderHealth(active?.id);
  if (measured === 0) return null;
  const running = (jobs.data ?? []).some(
    (j) => j.type === JOB_KINDS.describeVoice && (j.status === "queued" || j.status === "running"),
  );
  const where = active
    ? active.local
      ? `${active.displayName}, which runs on this computer`
      : `${active.displayName}, which is not on this computer`
    : null;

  return (
    <Card title="In words">
      <p>
        A model can read the numbers below and say in a sentence or two how you write &mdash; short
        and warm, say, or careful and formal. Drafts are given that reading beside the numbers, as a
        reading, until the numbers change.
      </p>
      {where && health.data?.reachable === false ? (
        <p className="muted small">
          {active?.displayName} isn&rsquo;t answering ({health.data.error ?? "no answer"}), so
          nothing can be put into words now.
        </p>
      ) : where ? (
        <div className="stack gap-1">
          <div className="row gap-2">
            <Button
              variant="secondary"
              disabled={start.isPending || running || health.data?.reachable !== true}
              onClick={() => start.mutate()}
            >
              {running ? "Putting it into words…" : "Put it into words"}
            </Button>
          </div>
          <p className="muted small">
            Only the numbers go to {where}, with the greetings and sign-offs you use from my own
            short list (&ldquo;hi&rdquo;, &ldquo;thanks&rdquo;) &mdash; never a message, a phrase
            you wrote, or who a layer is about.
          </p>
        </div>
      ) : (
        <p className="muted small">Set up a writing model in Settings first.</p>
      )}
    </Card>
  );
}
