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
import { ipc } from "@/lib/ipc";
import { useQueryClient } from "@tanstack/react-query";
import { qk } from "@/app/queryClient";

/**
 * Four steps, each gated on a fact rather than a checkbox: who you are, where
 * your messages come from, importing them, analyzing them. The step the user
 * is on is derived from the database, so closing the app mid-way resumes
 * exactly where it left off.
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
        <h1 className="onboarding__title">Mimic</h1>
        <p className="onboarding__lede">
          Mimic learns how you communicate from messages you have already written, and helps you
          draft replies that sound like yourself. Everything stays on this computer.
        </p>
        <p className="muted small">
          Only import conversations you own or have permission to process.
        </p>

        {step === "identity" ? (
          <IdentityStep onContinue={() => setIdentityConfirmed(true)} />
        ) : null}
        {step === "source" ? <SourceStep /> : null}
        {step === "import" ? <ImportStep /> : null}
        {step === "analyze" ? <AnalyzeStep onFinish={finish} /> : null}
        {step === null && onboarding.data ? (
          <Card title="Ready">
            <p>
              Mimic has read your messages and worked out how you write. The Compose screen is where
              you use it.
            </p>
            <Button variant="primary" onClick={finish}>
              Start using Mimic
            </Button>
          </Card>
        ) : null}

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
          Skip the rest and use Mimic now
        </Button>
      </p>
    );
  }
  if (!canLeaveOnboarding(state)) return null;
  return (
    <p className="muted small">
      <Button variant="ghost" size="sm" onClick={onLeave}>
        Look around first
      </Button>{" "}
      Mimic will be empty until you import something — the dashboard, People and Voice all have
      nothing to show yet. You can pick up setup again under Sources whenever you want.
    </p>
  );
}

/** Whatever is running right now, with its progress, so no step looks stuck. */
function ActiveWork() {
  const jobs = useJobs(true);
  const active = jobs.data ?? [];
  if (active.length === 0) return null;
  return (
    <Card title="Working">
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
    <Card title="First, which messages are yours">
      <p className="neutral">
        Mimic learns only from things you wrote. It tells them apart by the address they were sent
        from, so it needs to know yours — every address you have written from, or the messages sent
        from the missing ones will be read as someone else&rsquo;s.
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
              Added: {identifiers.map((i) => i.value).join(", ")}. You can add more later in
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
    <Card title="Now, where your messages live">
      <p className="neutral">
        Export your mail or messages from wherever they are and point Mimic at the file. A standard
        .mbox from Gmail Takeout or Thunderbird works; so does Mimic&rsquo;s own JSON format for
        anything else.
      </p>
      <Button variant="primary" onClick={() => setAdding(true)}>
        Choose a file
      </Button>
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
    <Card title="Last, work out how you write">
      <p className="neutral">
        Mimic reads back the messages you sent and measures them: how long they are, how you open
        and close, your punctuation, the phrases you repeat. This is arithmetic over your own text —
        nothing is sent anywhere.
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
