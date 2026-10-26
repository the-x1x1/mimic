import { useState } from "react";
import { Button, InlineError } from "@mimic/ui";
import { SmtpSecurity, type SmtpAccount } from "@mimic/contracts";
import {
  useMailboxSending,
  useSetMailboxSending,
  useSetMailboxSignature,
  useTestMailboxSending,
} from "@/hooks/useSending";
import { toast } from "@/state/toast";

const SECURITY_LABELS: Record<SmtpSecurity, string> = {
  tls: "TLS (usually port 465)",
  startTls: "STARTTLS (usually port 587)",
  plain: "None — a bridge on this computer only",
};

/**
 * How replies go out through a connected mailbox: the provider's usual
 * server, or one the user set. Changing it is checked before it is kept —
 * Mimic signs in to the server with the mailbox's own password or sign-in,
 * and sends nothing — so a setting that can't work is never saved.
 *
 * Its signature (0.10.0-alpha.36) is kept as given, with nothing tried:
 * it goes under each reply sent from the mailbox, after a `-- ` line.
 */
export function MailboxSending({ sourceId, name }: { sourceId: string; name: string }) {
  const current = useMailboxSending(sourceId);
  const save = useSetMailboxSending(sourceId);
  const test = useTestMailboxSending(sourceId);
  const signing = useSetMailboxSignature(sourceId);
  const [open, setOpen] = useState(false);
  const [signatureOpen, setSignatureOpen] = useState(false);
  const [signatureText, setSignatureText] = useState("");
  const signature = current.data?.signature ?? null;
  const sending = current.data?.sending ?? null;
  const [form, setForm] = useState<SmtpAccount | null>(null);

  if (current.isPending || current.isError) return null;

  function start() {
    setForm(
      sending ?? {
        host: "",
        port: 465,
        security: "tls",
        username: name,
        fromAddress: name.includes("@") ? name : "",
      },
    );
    save.reset();
    setOpen(true);
  }

  if (!open || !form) {
    return (
      <div className="stack gap-1">
        <p className="muted small">
          {sending
            ? `Replies go out through ${sending.host} as ${sending.fromAddress}${current.data?.setByUser ? "" : ", this provider's usual server"}${(sending.copyToSent ?? !current.data?.keepsSent) ? ", and I put a copy of each in your Sent folder" : ""}. `
            : "I can't send replies from this mailbox until you tell me its outgoing server. "}
          <button type="button" className="linkish" onClick={start}>
            {sending ? "Change" : "Set up sending"}
          </button>
          {sending ? (
            <>
              {" · "}
              <button
                type="button"
                className="linkish"
                disabled={test.isPending}
                onClick={() => test.mutate()}
              >
                {test.isPending ? "Trying…" : "Try it (sends nothing)"}
              </button>
            </>
          ) : null}
        </p>
        {test.isSuccess ? (
          <p className="muted small" role="status">
            I signed in to {test.data} and sent nothing: replies can go out from {name}.
          </p>
        ) : null}
        {test.isError ? <InlineError>{(test.error as Error).message}</InlineError> : null}
        {!sending ? null : signatureOpen ? (
          <form
            className="stack gap-1"
            aria-label={`Signature for ${name}`}
            onSubmit={(e) => {
              e.preventDefault();
              signing.mutate(signatureText, { onSuccess: () => setSignatureOpen(false) });
            }}
          >
            <label className="stack gap-1">
              <span className="muted small">Signature</span>
              <textarea
                rows={4}
                maxLength={1000}
                value={signatureText}
                onChange={(e) => {
                  signing.reset();
                  setSignatureText(e.target.value);
                }}
              />
            </label>
            <p className="muted small">
              It goes under each reply I send from {name}, after a &ldquo;--&rdquo; line, the way
              mail programs sign. It isn&rsquo;t part of what I learn from.
            </p>
            {signing.error ? <InlineError>{signing.error.message}</InlineError> : null}
            <div className="row gap-1">
              <Button size="sm" type="submit" disabled={signing.isPending}>
                {signing.isPending ? "Keeping…" : "Keep it"}
              </Button>
              {signature ? (
                <Button
                  size="sm"
                  variant="ghost"
                  type="button"
                  disabled={signing.isPending}
                  onClick={() => signing.mutate("", { onSuccess: () => setSignatureOpen(false) })}
                >
                  No signature
                </Button>
              ) : null}
              <Button
                size="sm"
                variant="ghost"
                type="button"
                onClick={() => setSignatureOpen(false)}
              >
                Never mind
              </Button>
            </div>
          </form>
        ) : (
          <div className="stack gap-1">
            {signature ? <p className="muted small">Replies from {name} are signed:</p> : null}
            {signature ? <p className="letter signature-preview">{signature}</p> : null}
            <p className="muted small">
              {signature ? null : "Replies go without a signature. "}
              <button
                type="button"
                className="linkish"
                onClick={() => {
                  setSignatureText(signature ?? "");
                  signing.reset();
                  setSignatureOpen(true);
                }}
              >
                {signature ? "Change the signature" : "Add a signature"}
              </button>
            </p>
          </div>
        )}
      </div>
    );
  }

  const set = <K extends keyof SmtpAccount>(key: K, value: SmtpAccount[K]) => {
    save.reset();
    setForm({ ...form, [key]: value });
  };

  return (
    <form
      className="stack gap-1"
      aria-label={`Sending from ${name}`}
      onSubmit={(e) => {
        e.preventDefault();
        save.mutate(form, {
          onSuccess: () => {
            setOpen(false);
            toast.success("Sending is set up", `Replies from ${name} go out through ${form.host}.`);
          },
        });
      }}
    >
      <label className="stack gap-1">
        <span className="muted small">Outgoing server</span>
        <input
          type="text"
          value={form.host}
          placeholder="smtp.example.com"
          onChange={(e) => set("host", e.target.value)}
        />
      </label>
      <div className="row gap-2">
        <label className="stack gap-1">
          <span className="muted small">Port</span>
          <input
            type="number"
            min={1}
            max={65535}
            value={form.port}
            onChange={(e) => set("port", Number(e.target.value))}
          />
        </label>
        <label className="stack gap-1">
          <span className="muted small">Encryption</span>
          <select
            value={form.security}
            onChange={(e) => set("security", SmtpSecurity.parse(e.target.value))}
          >
            {SmtpSecurity.options.map((s) => (
              <option key={s} value={s}>
                {SECURITY_LABELS[s]}
              </option>
            ))}
          </select>
        </label>
      </div>
      <label className="stack gap-1">
        <span className="muted small">Sign in as</span>
        <input
          type="text"
          value={form.username}
          onChange={(e) => set("username", e.target.value)}
        />
      </label>
      <label className="stack gap-1">
        <span className="muted small">Send from</span>
        <input
          type="email"
          value={form.fromAddress}
          onChange={(e) => set("fromAddress", e.target.value)}
        />
      </label>
      <label className="row gap-2">
        <input
          type="checkbox"
          checked={form.copyToSent ?? !(current.data?.keepsSent ?? false)}
          onChange={(e) => set("copyToSent", e.target.checked)}
        />
        <span>
          Put a copy of each reply in my Sent folder
          {current.data?.keepsSent ? " (this provider already does)" : ""}
        </span>
      </label>
      <p className="muted small">
        I sign in with this mailbox&rsquo;s own password or sign-in to check it, and send nothing.
      </p>
      {save.error ? <InlineError>{save.error.message}</InlineError> : null}
      <div className="row gap-1">
        <Button size="sm" type="submit" disabled={save.isPending || !form.host.trim()}>
          {save.isPending ? "Checking…" : "Check and keep"}
        </Button>
        {current.data?.setByUser ? (
          <Button
            size="sm"
            variant="ghost"
            type="button"
            disabled={save.isPending}
            onClick={() =>
              save.mutate(null, {
                onSuccess: () => setOpen(false),
              })
            }
          >
            Use the usual server
          </Button>
        ) : null}
        <Button size="sm" variant="ghost" type="button" onClick={() => setOpen(false)}>
          Never mind
        </Button>
      </div>
    </form>
  );
}
