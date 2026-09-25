import { useEffect, useRef, useState } from "react";
import { Button, Card, Field, InlineError, ProgressBar } from "@mimic/ui";
import {
  JOB_KINDS,
  JOB_LABELS,
  type OnboardingState,
  canFinishOnboarding,
  importStepState,
  nextOnboardingStep,
} from "@mimic/contracts";
import { useOnboardingState } from "@/hooks/useSystem";
import { useIdentity, useSetIdentity } from "@/hooks/usePeople";
import { useSources, useStartImport, useDeleteSource } from "@/hooks/useSources";
import { useStartAnalysis } from "@/hooks/useVoice";
import { useJobs } from "@/hooks/useJobs";
import { AddSourceDialog } from "@/features/sources/AddSourceDialog";
import { AddAddressForm, SentFolderNotice } from "@/features/identity/AddAddressForm";
import { ModelStep } from "./ModelStep";
import { ipc } from "@/lib/ipc";
import { useQueryClient } from "@tanstack/react-query";
import { qk } from "@/app/queryClient";

/**
 * Setting up, in as few decisions as it can be reduced to.
 *
 * The mail steps run in order, each gated on a fact about the database rather
 * than on a checkbox, so closing the app mid-way resumes exactly where it left
 * off. Getting the writing engine does not run in order: it is a download of a
 * couple of gigabytes, and making someone watch it before they are allowed to
 * go and export their mailbox wastes the one part of setup that takes real
 * time. So it sits below, running in parallel, and says where it has got to.
 *
 * Two things this screen must never do, both of which it used to: sit on a
 * step that a finished background job has already completed, and reach a state
 * with no button on it. Native job events are subscribed above this component,
 * and every step below has a way forward even when the import went wrong.
 */
export function OnboardingFlow() {
  const onboarding = useOnboardingState();
  const qc = useQueryClient();
  const derived = onboarding.data ? nextOnboardingStep(onboarding.data) : null;

  // Leave the name step explicitly. The backend's hasIdentity means it has
  // identifiers, which chat imports can establish later from their preview.
  const startedAtIdentity = useRef<boolean | null>(null);
  const [identityConfirmed, setIdentityConfirmed] = useState(false);
  const [started, setStarted] = useState(false);
  const [leaving, setLeaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    if (startedAtIdentity.current === null && derived !== null) {
      startedAtIdentity.current = derived === "identity";
    }
  }, [derived]);
  const holdIdentity = startedAtIdentity.current === true && !identityConfirmed;
  const step =
    holdIdentity && derived !== null
      ? "identity"
      : derived === "identity" && identityConfirmed
        ? onboarding.data?.hasSource
          ? "import"
          : "source"
        : derived;
  const welcome = onboarding.data && !onboarding.data.hasSource && !started;

  const finish = async () => {
    setLeaving(true);
    setError(null);
    try {
      await ipc.completeOnboarding();
      await qc.invalidateQueries({ queryKey: qk.onboarding });
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setLeaving(false);
    }
  };

  return (
    <div className="onboarding">
      <div className="onboarding__inner">
        <h1 className="onboarding__title">Welcome to Mimic.</h1>
        <p className="onboarding__lede">
          Bring your own messages. Mimic learns how you write and helps draft replies for you to
          review and send. Importing and measuring your writing happen on this computer.
        </p>
        <p className="muted small">
          No email connection required. Only import conversations you own or have permission to
          read.
        </p>
        {error ? <InlineError>{error}</InlineError> : null}
        {onboarding.isError ? <InlineError>{onboarding.error.message}</InlineError> : null}
        {onboarding.isPending ? <p className="muted">Loading your setup…</p> : null}
        {welcome ? (
          <Card title="Start with what you already have">
            <p>
              Import a WhatsApp chat, a Discord data package, an email export (.mbox), or a Mimic
              JSON file. You can start with one conversation and add more later.
            </p>
            <p className="muted small">
              Mimic learns your writing style from messages you wrote. It does not yet import PDFs,
              documents, or general reference knowledge.
            </p>
            <div className="row gap-2">
              <Button variant="primary" onClick={() => setStarted(true)}>
                Import my messages
              </Button>
              <Button disabled={leaving} onClick={finish}>
                Explore first
              </Button>
            </div>
            <p className="muted small">
              Exploring opens an empty workspace. Add messages and set up a writing model whenever
              you are ready in Settings.
            </p>
          </Card>
        ) : (
          <>
            {step === "identity" ? (
              <IdentityStep onContinue={() => setIdentityConfirmed(true)} />
            ) : null}
            {step === "source" ? <SourceStep /> : null}
            {step === "import" ? <ImportStep /> : null}
            {step === "analyze" ? <AnalyzeStep onFinish={finish} /> : null}
            {step === null && onboarding.data ? (
              <Card title="That&rsquo;s everything">
                <p>
                  I&rsquo;ve read your mail and worked out how you write. Let&rsquo;s see
                  who&rsquo;s waiting on you.
                </p>
                <Button variant="primary" onClick={finish}>
                  Take me in
                </Button>
              </Card>
            ) : null}

            <Card title="The part that does the writing">
              <p className="neutral">
                Set up a local writing model when you are ready to draft. Importing messages and
                measuring your writing do not need this download. You can also choose a provider in
                Settings; a hosted provider receives the context used for drafting.
              </p>
              <ModelStep />
            </Card>

            <ActiveWork />

            {onboarding.data && step !== null ? (
              <LeaveEarly state={onboarding.data} onLeave={finish} />
            ) : null}
          </>
        )}
      </div>
    </div>
  );
}

/**
 * The way out, on every step that has one.
 *
 * There are two of them and they are not the same offer. Someone whose
 * messages are already imported is being told they may skip the last step;
 * someone who has imported nothing is being told they may look at an empty
 * app, which is worth saying out loud rather than letting them find out.
 */
function LeaveEarly({ state, onLeave }: { state: OnboardingState; onLeave: () => void }) {
  if (canFinishOnboarding(state)) {
    return (
      <p className="muted small">
        <Button variant="ghost" size="sm" onClick={onLeave}>
          Skip the rest and take me in
        </Button>
      </p>
    );
  }
  return (
    <p className="muted small">
      <Button variant="ghost" size="sm" onClick={onLeave}>
        Have a look around first
      </Button>{" "}
      There won&rsquo;t be anything in there yet &mdash; no replies, nobody I&rsquo;ve met, nothing
      about how you write &mdash; but you can see where things are. Setup will be waiting under
      Settings.
    </p>
  );
}

/** Whatever is running right now, with its progress, so no step looks stuck. */
function ActiveWork() {
  const jobs = useJobs(true);
  const active = jobs.data ?? [];
  if (active.length === 0) return null;
  return (
    <Card title="Going on right now">
      {active.map((j) => (
        <ProgressBar
          key={j.id}
          current={j.progressCurrent}
          total={j.progressTotal}
          label={`${JOB_LABELS[j.type] ?? j.type} — ${j.status === "queued" ? "queued" : (j.phase ?? "working")}`}
        />
      ))}
    </Card>
  );
}

function IdentityStep({ onContinue }: { onContinue: () => void }) {
  const identity = useIdentity();
  const setIdentity = useSetIdentity();
  const [name, setName] = useState("");
  const identifiers = identity.data?.identifiers ?? [];

  return (
    <Card title="What should Mimic call you?">
      <p className="neutral">
        A name is enough to start. When you choose an export, you can tell Mimic which messages are
        yours. You do not need to connect an account.
      </p>
      {!identity.data ? (
        <>
          <Field label="Your name" htmlFor="ob-name">
            <input id="ob-name" value={name} onChange={(e) => setName(e.target.value)} autoFocus />
          </Field>
          <Button
            variant="primary"
            disabled={!name.trim() || setIdentity.isPending}
            onClick={() => setIdentity.mutate(name.trim(), { onSuccess: onContinue })}
          >
            Continue
          </Button>
        </>
      ) : (
        <>
          <p>Welcome, {identity.data.displayName}.</p>
          {identifiers.length > 0 ? (
            <p className="muted small">
              Added: {identifiers.map((i) => i.value).join(", ")}. You can add more later under
              Settings.
            </p>
          ) : null}
          <Button variant="primary" onClick={onContinue}>
            Continue to import
          </Button>
        </>
      )}
    </Card>
  );
}

function SourceStep() {
  const [adding, setAdding] = useState(false);
  return (
    <Card title="Choose messages to learn from">
      <p className="neutral">
        Start with a WhatsApp chat, Discord data package, email export (.mbox), or Mimic JSON file.
        You will see what is in the file before importing it. Nothing is uploaded.
      </p>
      <Button variant="primary" onClick={() => setAdding(true)}>
        Choose the file
      </Button>
      <p className="muted small">
        Choose a format to see what file it needs. Email exports need your sending address; chat
        exports let you identify yourself from the writers in the file.
      </p>
      {adding ? <AddSourceDialog onClose={() => setAdding(false)} /> : null}
    </Card>
  );
}

export function ImportStep() {
  const sources = useSources();
  const startImport = useStartImport();
  const deleteSource = useDeleteSource();
  const identity = useIdentity();
  // The last address added here that nothing already read came from.
  const [tried, setTried] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  const list = sources.data ?? [];
  const state = importStepState(list);
  const pending = list.find((s) => s.status !== "imported" && s.status !== "failed");
  const failed = list.find((s) => s.status === "failed");

  if (state === "running") {
    return (
      <Card title="Importing">
        <p className="neutral">
          Reading the file. Large mailboxes take a while; you can leave this running, and this
          screen will move on by itself when it finishes.
        </p>
      </Card>
    );
  }

  if (state === "failed") {
    return (
      <Card title="That import failed">
        <p className="neutral">{failed?.lastError?.message ?? "The source could not be read."}</p>
        <div className="row gap-2">
          {failed ? (
            <Button variant="primary" onClick={() => startImport.mutate(failed.id)}>
              Try again
            </Button>
          ) : null}
          {failed ? (
            <Button variant="ghost" onClick={() => deleteSource.mutate(failed.id)}>
              Remove it and choose another file
            </Button>
          ) : null}
        </div>
      </Card>
    );
  }

  if (state === "none-of-yours") {
    // The mail was read and not one message in it matched an address Mimic
    // knows about. Saying "importing" here, forever, was the single worst
    // thing this screen did. What was written from the missing address was
    // filed under a person; adding it asks whether that person is the user
    // and moves their messages over, so nothing needs reading again.
    return (
      <Card title="Nothing I read was written by you">
        <p className="neutral">
          I read it, but not one message came from{" "}
          {identity.data?.identifiers.map((i) => i.value).join(", ") || "any address you gave me"}.
          I only learn from mail you wrote, so I have nothing to go on yet. Usually that means you
          wrote it from another address &mdash; add it here and I&rsquo;ll count what came from it
          as yours.
        </p>
        {/* When the mail says which address that was, ask about it first. */}
        <SentFolderNotice open />
        <AddAddressForm
          primary
          inputLabel="Another address of yours"
          placeholder="another address of yours"
          submitLabel="Add it"
          onAdded={(added, address, asked) =>
            setTried(!asked && added.claimed.messages === 0 ? address : null)
          }
        />
        {tried ? (
          <p className="muted small">
            Nothing I&rsquo;ve read came from {tried} either. If you wrote from another address, add
            that one too.
          </p>
        ) : null}
        <div className="row gap-2">
          <Button variant="ghost" onClick={() => setAdding(true)}>
            Choose a different file
          </Button>
          {list[0] ? (
            <Button variant="ghost" onClick={() => deleteSource.mutate(list[0]!.id)}>
              Remove this source
            </Button>
          ) : null}
        </div>
        {adding ? <AddSourceDialog onClose={() => setAdding(false)} /> : null}
      </Card>
    );
  }

  return (
    <Card title="Importing">
      <p className="neutral">
        Reading the file. Large mailboxes take a while; you can leave this running.
      </p>
      {pending ? (
        <Button
          variant="primary"
          onClick={() => startImport.mutate(pending.id)}
          disabled={startImport.isPending}
        >
          Start the import
        </Button>
      ) : (
        <Button variant="primary" onClick={() => setAdding(true)}>
          Choose a file
        </Button>
      )}
      {adding ? <AddSourceDialog onClose={() => setAdding(false)} /> : null}
    </Card>
  );
}

function AnalyzeStep({ onFinish }: { onFinish: () => void }) {
  const analyze = useStartAnalysis();
  const jobs = useJobs(true);
  const running = (jobs.data ?? []).some((j) => j.type === JOB_KINDS.analyzeVoice);
  const [tried, setTried] = useState(false);
  return (
    <Card title="Last, let me read it">
      <p className="neutral">
        I read back the mail you sent and count things: how long your messages are, how you open and
        close, your punctuation, the phrases you come back to. It is arithmetic over your own words
        and nothing is sent anywhere.
      </p>
      <Button
        variant="primary"
        onClick={() => {
          setTried(true);
          analyze.mutate();
        }}
        disabled={analyze.isPending || running}
      >
        {running ? "Analyzing…" : "Analyze my messages"}
      </Button>
      {tried && !running && !analyze.isPending ? (
        <>
          <p className="muted small">
            If this step keeps coming back, Mimic has fewer than twenty messages of yours to measure
            — not enough to describe a style honestly. You can use it anyway; Compose will say when
            it is drafting without one.
          </p>
          <Button variant="ghost" onClick={onFinish}>
            Use Mimic anyway
          </Button>
        </>
      ) : null}
    </Card>
  );
}
