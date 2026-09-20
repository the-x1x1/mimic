import { useState } from "react";
import { Button, Card, Field, InlineError } from "@mimic/ui";
import { IDENTIFIER_LABELS, IdentifierKind, nextOnboardingStep } from "@mimic/contracts";
import { useOnboardingState } from "@/hooks/useSystem";
import { useAddIdentifier, useIdentity, useSetIdentity } from "@/hooks/usePeople";
import { useSources, useStartImport } from "@/hooks/useSources";
import { useStartAnalysis } from "@/hooks/useVoice";
import { AddSourceDialog } from "@/features/sources/AddSourceDialog";
import { ipc } from "@/lib/ipc";
import { useQueryClient } from "@tanstack/react-query";
import { qk } from "@/app/queryClient";

/**
 * Four steps, each gated on a fact rather than a checkbox: who you are, where
 * your messages come from, importing them, analyzing them. The step the user
 * is on is derived from the database, so closing the app mid-way resumes
 * exactly where it left off.
 */
export function OnboardingFlow() {
  const onboarding = useOnboardingState();
  const qc = useQueryClient();
  const step = onboarding.data ? nextOnboardingStep(onboarding.data) : null;

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

        {step === "identity" ? <IdentityStep /> : null}
        {step === "source" ? <SourceStep /> : null}
        {step === "import" ? <ImportStep /> : null}
        {step === "analyze" ? <AnalyzeStep /> : null}
        {step === null && onboarding.data ? (
          <Card title="Ready">
            <p>
              Mimic has read your messages and worked out how you write. The Compose screen is where
              you use it.
            </p>
            <Button
              variant="primary"
              onClick={async () => {
                await ipc.completeOnboarding();
                qc.invalidateQueries({ queryKey: qk.onboarding });
              }}
            >
              Start using Mimic
            </Button>
          </Card>
        ) : null}
      </div>
    </div>
  );
}

function IdentityStep() {
  const identity = useIdentity();
  const setIdentity = useSetIdentity();
  const add = useAddIdentifier();
  const [name, setName] = useState("");
  const [kind, setKind] = useState<IdentifierKind>("email");
  const [value, setValue] = useState("");

  return (
    <Card title="First, which messages are yours">
      <p className="neutral">
        Mimic learns only from things you wrote. It tells them apart by the address they were sent
        from, so it needs to know yours.
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
              disabled={!value.trim()}
              onClick={async () => {
                await add.mutateAsync({ kind, value });
                setValue("");
              }}
            >
              Add
            </Button>
          </div>
          {identity.data.identifiers.length > 0 ? (
            <p className="muted small">
              Added: {identity.data.identifiers.map((i) => i.value).join(", ")}. You can add more
              later in Settings.
            </p>
          ) : null}
          {add.isError ? <InlineError>{(add.error as Error).message}</InlineError> : null}
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
  const pending = sources.data?.find((s) => s.messageCount === 0);
  return (
    <Card title="Importing">
      <p className="neutral">
        Reading the file. Large mailboxes take a while; you can leave this running.
      </p>
      {pending ? (
        <Button
          variant="primary"
          onClick={() => startImport.mutate(pending.id)}
          disabled={startImport.isPending || pending.status === "importing"}
        >
          {pending.status === "importing" ? "Importing…" : "Start the import"}
        </Button>
      ) : null}
    </Card>
  );
}

function AnalyzeStep() {
  const analyze = useStartAnalysis();
  return (
    <Card title="Last, work out how you write">
      <p className="neutral">
        Mimic reads back the messages you sent and measures them: how long they are, how you open
        and close, your punctuation, the phrases you repeat. This is arithmetic over your own text —
        nothing is sent anywhere.
      </p>
      <Button variant="primary" onClick={() => analyze.mutate()} disabled={analyze.isPending}>
        {analyze.isPending ? "Starting…" : "Analyze my messages"}
      </Button>
    </Card>
  );
}
