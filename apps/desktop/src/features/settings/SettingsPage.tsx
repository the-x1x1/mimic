import { useState } from "react";
import { Badge, Button, Card, Field, InlineError } from "@mimic/ui";
import { ANTHROPIC_SECRET_KEY, IDENTIFIER_LABELS, IdentifierKind } from "@mimic/contracts";
import { PageHeader } from "@/components/PageHeader";
import { useAppInfo, useSettings } from "@/hooks/useSystem";
import {
  useAddIdentifier,
  useIdentity,
  useRemoveIdentifier,
  useSetIdentity,
} from "@/hooks/usePeople";
import { useProviderState } from "@/hooks/useCompose";
import { ipc } from "@/lib/ipc";
import { toast } from "@/state/toast";
import { useQueryClient } from "@tanstack/react-query";
import { qk } from "@/app/queryClient";
import { DeleteEverythingDialog } from "./DeleteEverythingDialog";

export function SettingsPage() {
  return (
    <div className="stack gap-3">
      <PageHeader title="Settings" />
      <IdentitySection />
      <ProviderSection />
      <PrivacySection />
      <AboutSection />
    </div>
  );
}

/**
 * Who the user is. This is not a profile page: these addresses are what
 * decides whether an imported message counts as the user's own writing, so the
 * section says so.
 */
function IdentitySection() {
  const identity = useIdentity();
  const setIdentity = useSetIdentity();
  const add = useAddIdentifier();
  const remove = useRemoveIdentifier();
  const [kind, setKind] = useState<IdentifierKind>("email");
  const [value, setValue] = useState("");

  return (
    <Card title="You">
      <p className="neutral">
        Mimic learns only from messages you wrote. It works out which those are by matching the
        sender against the addresses below — so if one is missing, the messages you sent from it
        will be treated as someone else&rsquo;s.
      </p>
      <Field label="Your name" htmlFor="display-name">
        <input
          id="display-name"
          defaultValue={identity.data?.displayName ?? ""}
          placeholder="How you sign off"
          onBlur={(e) => e.target.value.trim() && setIdentity.mutate(e.target.value)}
        />
      </Field>

      {identity.data ? (
        <ul className="plain-list">
          {identity.data.identifiers.map((i) => (
            <li key={i.id} className="row gap-2 between">
              <span>
                <span className="muted small">{IDENTIFIER_LABELS[i.kind as IdentifierKind]}</span>{" "}
                <span className="mono">{i.value}</span>
              </span>
              <Button size="sm" variant="ghost" onClick={() => remove.mutate(i.id)}>
                Remove
              </Button>
            </li>
          ))}
        </ul>
      ) : null}

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
        />
        <Button
          disabled={!identity.data || !value.trim()}
          onClick={async () => {
            await add.mutateAsync({ kind, value });
            setValue("");
          }}
        >
          Add
        </Button>
      </div>
      {!identity.data ? <p className="muted small">Set your name first.</p> : null}
      {add.isError ? <InlineError>{(add.error as Error).message}</InlineError> : null}
    </Card>
  );
}

/** Where drafts are written, and what that means for the user's messages. */
function ProviderSection() {
  const providers = useProviderState();
  const settings = useSettings();
  const qc = useQueryClient();
  const [key, setKey] = useState("");
  const [checking, setChecking] = useState<string | null>(null);

  async function check(id: string) {
    setChecking(id);
    try {
      await ipc.checkProvider(id);
      toast.success("That provider answered");
    } catch (e) {
      toast.danger("No answer from that provider", (e as Error).message);
    } finally {
      setChecking(null);
    }
  }

  return (
    <Card title="Model">
      {(providers.data?.providers ?? []).map((p) => (
        <div key={p.id} className="provider">
          <div className="row gap-2 between">
            <label className="row gap-2">
              <input
                type="radio"
                name="provider"
                checked={providers.data?.active === p.id}
                onChange={async () => {
                  await ipc.setActiveProvider(p.id);
                  qc.invalidateQueries({ queryKey: qk.providers });
                  qc.invalidateQueries({ queryKey: qk.settings });
                }}
              />
              <span>
                {p.displayName} <span className="muted small mono">{p.model}</span>
              </span>
            </label>
            <div className="row gap-2">
              <Badge tone={p.local ? "success" : "warning"}>
                {p.local ? "Stays on this computer" : "Sends your messages out"}
              </Badge>
              <Button
                size="sm"
                variant="ghost"
                onClick={() => check(p.id)}
                disabled={checking === p.id}
              >
                {checking === p.id ? "Checking…" : "Test"}
              </Button>
            </div>
          </div>
          <p className="muted small">{p.description}</p>
          {p.requiresCredential ? (
            <div className="row gap-2">
              <input
                type="password"
                placeholder={
                  providers.data?.configuredSecrets.includes(ANTHROPIC_SECRET_KEY)
                    ? "A key is saved — type a new one to replace it"
                    : "API key"
                }
                value={key}
                onChange={(e) => setKey(e.target.value)}
              />
              <Button
                size="sm"
                onClick={async () => {
                  await ipc.setProviderSecret(ANTHROPIC_SECRET_KEY, key);
                  setKey("");
                  qc.invalidateQueries({ queryKey: qk.providers });
                  toast.success("Saved");
                }}
              >
                Save
              </Button>
            </div>
          ) : null}
        </div>
      ))}
      <Field
        label="Local endpoint"
        hint="Anything that speaks the OpenAI chat API: Ollama, LM Studio, llama.cpp."
      >
        <input
          defaultValue={settings.data?.["generation.localUrl"] ?? ""}
          onBlur={(e) => ipc.setSetting("generation.localUrl", e.target.value)}
        />
      </Field>
      <Field label="Local model">
        <input
          defaultValue={settings.data?.["generation.localModel"] ?? ""}
          onBlur={(e) => ipc.setSetting("generation.localModel", e.target.value)}
        />
      </Field>
    </Card>
  );
}

function PrivacySection() {
  const settings = useSettings();
  const [confirming, setConfirming] = useState(false);
  return (
    <Card title="Privacy">
      <p className="neutral">
        Your messages are stored in a database on this computer and are never uploaded. The one
        exception is drafting: the provider you chose above sees the message you are replying to,
        your intent, and a handful of your own past messages.
      </p>
      <label className="row gap-2">
        <input
          type="checkbox"
          checked={settings.data?.["diagnostics.includePaths"] ?? false}
          onChange={(e) => ipc.setSetting("diagnostics.includePaths", e.target.checked)}
        />
        <span>Include full file paths in diagnostics bundles</span>
      </label>
      <div className="row gap-2">
        <Button variant="danger" onClick={() => setConfirming(true)}>
          Delete everything Mimic has imported
        </Button>
      </div>
      {confirming ? <DeleteEverythingDialog onClose={() => setConfirming(false)} /> : null}
    </Card>
  );
}

function AboutSection() {
  const info = useAppInfo();
  const settings = useSettings();
  return (
    <Card title="About">
      <dl className="kv">
        <dt>Version</dt>
        <dd className="mono">{info.data?.version}</dd>
        <dt>Database schema</dt>
        <dd className="mono">v{info.data?.schemaVersion}</dd>
        <dt>Voice analysis</dt>
        <dd className="mono">{info.data?.analysisVersion}</dd>
        <dt>Data folder</dt>
        <dd className="mono">{info.data?.dataRoot}</dd>
      </dl>
      <label className="row gap-2">
        <input
          type="checkbox"
          checked={settings.data?.["updates.automatic"] ?? true}
          onChange={(e) => ipc.setSetting("updates.automatic", e.target.checked)}
        />
        <span>Check for updates automatically</span>
      </label>
      <Button size="sm" variant="ghost" onClick={() => ipc.openLogsFolder()}>
        Open logs folder
      </Button>
    </Card>
  );
}
