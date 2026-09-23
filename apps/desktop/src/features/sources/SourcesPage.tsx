import { useState } from "react";
import { Badge, Button, Card, EmptyState } from "@mimic/ui";
import { CHANNEL_LABELS, type Channel } from "@mimic/contracts";
import { PageHeader } from "@/components/PageHeader";
import { useDeleteSource, useSources, useStartImport } from "@/hooks/useSources";
import { AddSourceDialog } from "./AddSourceDialog";
import { ConnectMailboxDialog } from "./ConnectMailboxDialog";
import { MailboxPassword } from "./MailboxPassword";
import { formatRelative } from "@/lib/format";

const STATUS_TONE: Record<string, "success" | "warning" | "danger" | "neutral"> = {
  imported: "success",
  importing: "warning",
  failed: "danger",
  ready: "neutral",
  new: "neutral",
};

export function SourcesPage() {
  const sources = useSources();
  const startImport = useStartImport();
  const remove = useDeleteSource();
  const [adding, setAdding] = useState(false);
  const [connecting, setConnecting] = useState(false);
  const hasMailbox = (sources.data ?? []).some((s) => s.connector === "imap");

  return (
    <div className="stack gap-3">
      <PageHeader
        title="Your mail"
        subtitle={
          hasMailbox
            ? "Everything I know comes from mail you gave me: files you handed over, and the mailbox you connected, which I check for new mail. Nothing is uploaded."
            : "Everything I know comes from files you hand me. Nothing is uploaded, and nothing arrives on its own."
        }
        actions={
          <div className="row gap-2">
            <Button onClick={() => setConnecting(true)}>Connect a mailbox</Button>
            <Button variant="primary" onClick={() => setAdding(true)}>
              Add a file
            </Button>
          </div>
        }
      />

      {sources.isSuccess && sources.data.length === 0 ? (
        <EmptyState
          title="You haven't given me any mail yet"
          body="Connect your mailbox and I'll read your inbox and sent mail, or export your mail and point me at the file. Only bring in conversations you own or have permission to process."
          primary={
            <div className="row gap-2">
              <Button variant="primary" onClick={() => setConnecting(true)}>
                Connect a mailbox
              </Button>
              <Button onClick={() => setAdding(true)}>Add a file</Button>
            </div>
          }
        />
      ) : (
        <Card>
          <table className="table">
            <thead>
              <tr>
                <th>Source</th>
                <th>Channel</th>
                <th className="num">Messages</th>
                <th>Last import</th>
                <th>Status</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {(sources.data ?? []).map((s) => (
                <tr key={s.id}>
                  <td>
                    <div>{s.name}</div>
                    <div className="muted small mono">
                      {s.connector === "imap"
                        ? `${String((s.config as { account?: { host?: string } }).account?.host ?? "mailbox")} · checked ${formatRelative((s.config as { lastCheckedAt?: string | null }).lastCheckedAt ?? null)}`
                        : (s.location ?? s.connector)}
                    </div>
                    {s.lastError ? (
                      <div className="danger small">{String(s.lastError.message)}</div>
                    ) : null}
                  </td>
                  <td>{CHANNEL_LABELS[s.channel as Channel] ?? s.channel}</td>
                  <td className="num">{s.messageCount.toLocaleString()}</td>
                  <td className="muted small">{formatRelative(s.lastImportedAt)}</td>
                  <td>
                    <Badge tone={STATUS_TONE[s.status] ?? "neutral"}>{s.status}</Badge>
                  </td>
                  <td>
                    <div className="row gap-1">
                      <Button
                        size="sm"
                        onClick={() => startImport.mutate(s.id)}
                        disabled={s.status === "importing" || startImport.isPending}
                      >
                        {s.connector === "imap"
                          ? "Check now"
                          : s.messageCount > 0
                            ? "Import again"
                            : "Import"}
                      </Button>
                      <Button size="sm" variant="ghost" onClick={() => remove.mutate(s.id)}>
                        Remove
                      </Button>
                    </div>
                    {s.connector === "imap" ? (
                      <MailboxPassword sourceId={s.id} name={s.name} />
                    ) : null}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          <p className="muted small">
            Re-importing the same file adds nothing: messages are identified by the source they came
            from and their own id, so a second run finds only duplicates.
          </p>
        </Card>
      )}

      {adding ? <AddSourceDialog onClose={() => setAdding(false)} /> : null}
      {connecting ? <ConnectMailboxDialog onClose={() => setConnecting(false)} /> : null}
    </div>
  );
}
