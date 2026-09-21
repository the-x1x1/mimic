import { Button, InlineError, ProgressBar } from "@mimic/ui";
import { useLocalModel, useStartModelPull } from "@/hooks/useDashboard";
import { useJobs } from "@/hooks/useJobs";
import { ipc } from "@/lib/ipc";
import { describeDownload } from "@/lib/format";

/**
 * The step that used to lose everybody: "point Mimic at an OpenAI-compatible
 * endpoint on 127.0.0.1".
 *
 * Nobody is asked what Ollama is. There are three states and each one has a
 * single button. Mimic will not download and run an installer itself — see the
 * note at the top of `localmodel.rs` for why — so getting the host is a link
 * to its own download page, after which Mimic watches for it to appear and
 * takes over again by itself. The model download after that is Mimic's job and
 * it does the whole thing, with a real progress figure.
 */
export function ModelStep({ onReady }: { onReady?: () => void }) {
  const model = useLocalModel(true);
  const pull = useStartModelPull();
  const jobs = useJobs(true);
  const running = (jobs.data ?? []).find((j) => j.type === "pull_model");

  if (model.isError) {
    return <InlineError>{(model.error as Error).message}</InlineError>;
  }
  if (!model.data) {
    return <p className="muted">Having a look at what&rsquo;s on this computer&hellip;</p>;
  }

  if (running) {
    return (
      <div className="stack gap-3">
        <ProgressBar
          current={running.progressCurrent}
          total={running.progressTotal}
          label={describeDownload(running.progressCurrent, running.progressTotal)}
        />
        <div className="row gap-3">
          <span className="muted small spacer">
            You can leave this running and come back &mdash; it carries on without this window.
          </span>
          <Button size="sm" onClick={() => ipc.cancelJob(running.id)}>
            Stop
          </Button>
        </div>
      </div>
    );
  }

  if (model.data.nextStep === "ready") {
    return (
      <div className="stack gap-3">
        <p>
          Ready. <strong>{model.data.wanted}</strong> is on this computer and answering, so
          everything I write for you is written here.
        </p>
        {onReady ? (
          <div>
            <Button variant="primary" onClick={onReady}>
              Finish
            </Button>
          </div>
        ) : null}
      </div>
    );
  }

  if (model.data.nextStep === "getTheHost") {
    return (
      <div className="stack gap-3">
        <p className="muted">
          Nothing on this computer is set up to write yet. The free program below is what does the
          actual writing &mdash; install it, leave it running, and I&rsquo;ll notice and carry on
          from here by myself.
        </p>
        <div className="row gap-3 wrap">
          <a href={model.data.downloadPage} target="_blank" rel="noreferrer">
            <Button variant="primary">Get it (opens your browser)</Button>
          </a>
          <span className="muted small">
            I&rsquo;m checking every few seconds. Nothing is downloaded or run by me &mdash; you
            install it yourself.
          </span>
        </div>
      </div>
    );
  }

  return (
    <div className="stack gap-3">
      <p className="muted">
        Good &mdash; that&rsquo;s running. Now it needs the model itself:{" "}
        <strong>{model.data.wanted}</strong>, about two gigabytes, downloaded once and then yours.
        I&rsquo;ll fetch it.
      </p>
      <div className="row gap-3 wrap">
        <Button variant="primary" disabled={pull.isPending} onClick={() => pull.mutate()}>
          {pull.isPending ? "Starting…" : "Download it"}
        </Button>
        <span className="muted small">
          It comes from the program you just installed, not from me, and it stays on this computer.
        </span>
      </div>
      {model.data.error ? (
        <p className="muted small">
          It answered, but I couldn&rsquo;t read the reply: {model.data.error}
        </p>
      ) : null}
    </div>
  );
}
