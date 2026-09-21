import { useEffect, useRef, useState } from "react";
import { Button, Card, Field, InlineError, ProgressBar } from "@mimic/ui";
import {
  IDENTIFIER_LABELS,
  IdentifierKind,
  JOB_LABELS,
  type OnboardingState,
  canFinishOnboarding,
  canLeaveOnboarding,
  importStepState,
  nextOnboardingStep,
} from "@mimic/contracts";
import { useOnboardingState } from "@/hooks/useSystem";
import { useAddIdentifier, useIdentity, useSetIdentity } from "@/hooks/usePeople";
import { useSources, useStartImport, useDeleteSource } from "@/hooks/useSources";
import { useStartAnalysis } from "@/hooks/useVoice";
import { useJobs } from "@/hooks/useJobs";
import { AddSourceDialog } from "@/features/sources/AddSourceDialog";
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

  // Adding one address should not throw the user forward to the next step
  // while they are still typing the second one, so the identity step is left
  // behind on a click rather than on a fact — but only for a user who started
  // there in this session.
  const startedAtIdentity = useRef<boolean | null>(null);
  const [identityConfirmed, setIdentityConfirmed] = useState(false);
  useEffect(() => {
    if (startedAtIdentity.current === null && derived !== null) {
      startedAtIdentity.current = derived === "identity";
    }
  }, [derived]);
  const holdIdentity = startedAtIdentity.current === true && !identityConfirmed;
  const step = holdIdentity && derived !== null ? "identity" : derived;

  const finish = async () => {
    await ipc.completeOnboarding();
    qc.invalidateQueries({ queryKey: qk.onboarding });
  };

  return (
    <div className="onboarding">
      <div className="onboarding__inner">
        <h1 className="onboarding__title">Let&rsquo;s get you set up.</h1>
        <p className="onboarding__lede">
          I learn how you write by reading mail you&rsquo;ve already sent, and then I draft your
          replies for you to check and send. Everything stays on this computer.
        </p>
        <p className="muted small">
          Only bring in mail that is yours, or that you have permission to read.
        </p>

        {step === "identity" ? (
          <IdentityStep onContinue={() => setIdentityConfirmed(true)} />
        ) : null}
        {step === "source" ? <SourceStep /> : null}
        {step === "import" ? <ImportStep /> : null}
        {step === "analyze" ? <AnalyzeStep onFinish={finish} /> : null}
        {step === null && onboarding.data ? (
          <Card title="That&rsquo;s everything">
            <p>
              I&rsquo;ve read your mail and worked out how you write. Let&rsquo;s see who&rsquo;s
              waiting on you.
            </p>
            <Button variant="primary" onClick={finish}>
              Take me in
            </Button>
          </Card>
        ) : null}

        <Card title="The part that does the writing">
          <p className="neutral">
            This runs on your computer rather than someone else&rsquo;s, which is why your mail
            never leaves it. You only do this once, and you can get on with the steps above while it
            happens.
          </p>
          <ModelStep />
        </Card>

        <ActiveWork />

        {onboarding.data && step !== null ? (
          <LeaveEarly state={onboarding.data} onLeave={finish} />
        ) : null}
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
  if (!canLeaveOnboarding(state)) return null;
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
  const add = useAddIdentifier();
  const [name, setName] = useState("");
  const [kind, setKind] = useState<IdentifierKind>("email");
  const [value, setValue] = useState("");
  const identifiers = identity.data?.identifiers ?? [];

  return (
    <Card title="First, your email address">
      <p className="neutral">
        This is how I tell the mail you wrote apart from the mail you were sent &mdash; I only learn
        from yours. Add every address you have written from, or the mail you sent from the missing
        ones will read to me like someone else&rsquo;s.
      </p>
      {!identity.data ? (
        <>
          <Field label="Your name" htmlFor="ob-name">
            <input id="ob-name" value={name} onChange={(e) => setName(e.target.value)} autoFocus />
          </Field>
          <Button
            variant="primary"
            disabled={!name.trim()}
            onClick={() => setIdentity.mutate(name)}
          >
            Continue
          </Button>
        </>
      ) : (
        <>
          <div className="row gap-2">
            <select value={kind} onChange={(e) => setKind(e.target.value as IdentifierKind)}>
              {IdentifierKind.options.map((k) => (
                <option key={k} value={k}>
                  {IDENTIFIER_LABELS[k]}
                </option>
              ))}
            </select>
            <input
              value={value}
              placeholder="you@example.com"
              onChange={(e) => setValue(e.target.value)}
              autoFocus
            />
            <Button
              variant="primary"
              disabled={!value.trim() || add.isPending}
              onClick={async () => {
                await add.mutateAsync({ kind, value });
                setValue("");
              }}
            >
              Add
            </Button>
          </div>
          {identifiers.length > 0 ? (
            <p className="muted small">
              Added: {identifiers.map((i) => i.value).join(", ")}. You can add more later under
              Settings.
            </p>
          ) : null}
          {add.isError ? <InlineError>{(add.error as Error).message}</InlineError> : null}
          <Button variant="primary" disabled={identifiers.length === 0} onClick={onContinue}>
            {identifiers.length === 0 ? "Add an address to continue" : "Continue"}
          </Button>
        </>
      )}
    </Card>
  );
}

function SourceStep() {
  const [adding, setAdding] = useState(false);
  return (
    <Card title="Now, your old mail">
      <p className="neutral">
        Save a copy of your mailbox and point me at the file. Gmail, Outlook and Thunderbird can all
        do this. I read it here and nothing is uploaded.
      </p>
      <Button variant="primary" onClick={() => setAdding(true)}>
        Choose the file
      </Button>
      <p className="muted small">
        Not sure how? In Gmail it&rsquo;s Google Takeout; in Thunderbird, right-click the folder and
        choose Export. Either gives you one file.
      </p>
      {adding ? <AddSourceDialog onClose={() => setAdding(false)} /> : null}
    </Card>
  );
}

function ImportStep() {
  const sources = useSources();
  const startImport = useStartImport();
  const deleteSource = useDeleteSource();
  const identity = useIdentity();
  const add = useAddIdentifier();
  const [kind, setKind] = useState<IdentifierKind>("email");
  const [value, setValue] = useState("");
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
    // The file was read and not one message in it matched an address Mimic
    // knows about. Saying "importing" here, forever, was the single worst
    // thing this screen did.
    return (
      <Card title="Nothing in that file was written by you">
        <p className="neutral">
          Mimic read the file but could not find a single message sent from{" "}
          {identity.data?.identifiers.map((i) => i.value).join(", ") || "any address you gave it"}.
          It only learns from messages you wrote, so it has nothing to work with yet. Usually this
          means the export was sent from another address.
        </p>
        <div className="row gap-2">
          <select value={kind} onChange={(e) => setKind(e.target.value as IdentifierKind)}>
            {IdentifierKind.options.map((k) => (
              <option key={k} value={k}>
                {IDENTIFIER_LABELS[k]}
              </option>
            ))}
          </select>
          <input
            value={value}
            placeholder="another address of yours"
            onChange={(e) => setValue(e.target.value)}
          />
          <Button
            variant="primary"
            disabled={!value.trim() || add.isPending}
            onClick={async () => {
              await add.mutateAsync({ kind, value });
              setValue("");
              const again = list.find((s) => s.status === "imported");
              if (again) startImport.mutate(again.id);
            }}
          >
            Add it and read the file again
          </Button>
        </div>
        {add.isError ? <InlineError>{(add.error as Error).message}</InlineError> : null}
        <p className="muted small">
          Re-reading a file you have already imported costs nothing: messages are matched on their
          own identifiers, so nothing is duplicated.
        </p>
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
  const running = (jobs.data ?? []).some((j) => j.type === "analyze");
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
