import { useState } from "react";
import { Badge, Button, Card, EmptyState } from "@mimic/ui";
import {
  CHANNEL_LABELS,
  googleChatAccount,
  mailboxAuth,
  slackWorkspace,
  teamsAccount,
  telegramAccount,
  type Channel,
  type Source,
} from "@mimic/contracts";
import { PageHeader } from "@/components/PageHeader";
import { useDeleteSource, useSources, useStartImport } from "@/hooks/useSources";
import { AddSourceDialog } from "./AddSourceDialog";
import { ConnectMailboxDialog } from "./ConnectMailboxDialog";
import { ConnectSlackDialog, SlackToken } from "./ConnectSlackDialog";
import { ConnectTeamsDialog, TeamsSignInAgain } from "./ConnectTeamsDialog";
import {
  ConnectGoogleChatDialog,
  GoogleChatSignInAgain,
  GoogleChatSpaces,
} from "./ConnectGoogleChatDialog";
import { ConnectTelegramDialog } from "./ConnectTelegramDialog";
import { MailboxPassword, MailboxSignInAgain } from "./MailboxPassword";
import { MailboxFolders } from "./MailboxFolders";
import { MailboxSending } from "./MailboxSending";
import { SentList } from "./SentList";
import { ServerTranscript } from "./ServerTranscript";
import { formatRelative } from "@/lib/format";

const STATUS_TONE: Record<string, "success" | "warning" | "danger" | "neutral"> = {
  imported: "success",
  importing: "warning",
  failed: "danger",
  ready: "neutral",
  new: "neutral",
};

/** Under a source's name: where it is read from, and when last, for a live one. */
function whereFrom(s: Source): string {
  if (s.connector === "imap") {
    const host = (s.config as { account?: { host?: string } }).account?.host ?? "mailbox";
    const checked = (s.config as { lastCheckedAt?: string | null }).lastCheckedAt ?? null;
    return `${String(host)} · checked ${formatRelative(checked)}`;
  }
  const slack = slackWorkspace(s);
  if (slack) return `Slack, as ${slack.user} · checked ${formatRelative(slack.lastCheckedAt)}`;
  const teams = teamsAccount(s);
  if (teams) return `Teams, as ${teams.user} · checked ${formatRelative(teams.lastCheckedAt)}`;
  const chat = googleChatAccount(s);
  if (chat) {
    return `Google Chat, as ${chat.email ?? chat.user} · checked ${formatRelative(chat.lastCheckedAt)}`;
  }
  const telegram = telegramAccount(s);
  if (telegram) {
    const who = telegram.username ? `@${telegram.username}` : telegram.name;
    return `Telegram, as ${who} · checked ${formatRelative(telegram.lastCheckedAt)}`;
  }
  return s.location ?? s.connector;
}

export function SourcesPage() {
  const sources = useSources();
  const startImport = useStartImport();
  const remove = useDeleteSource();
  const [adding, setAdding] = useState(false);
  const [connecting, setConnecting] = useState(false);
  const [connectingSlack, setConnectingSlack] = useState(false);
  const [connectingTeams, setConnectingTeams] = useState(false);
  const [connectingChat, setConnectingChat] = useState(false);
  const [connectingTelegram, setConnectingTelegram] = useState(false);
  const hasMailbox = (sources.data ?? []).some((s) => s.connector === "imap");

  return (
    <div className="stack gap-3">
      <PageHeader
        title="Your mail"
        subtitle={
          hasMailbox
            ? "Everything I know comes from mail you gave me: files you handed over, and the mailbox you connected, which I check for new mail. Nothing is uploaded. A reply you send from me goes out through that mailbox, as if you'd sent it yourself."
            : "Everything I know comes from files you hand me. Nothing is uploaded, and nothing arrives on its own."
        }
        actions={
          <div className="row gap-2">
            <Button onClick={() => setConnecting(true)}>Connect a mailbox</Button>
            <Button onClick={() => setConnectingSlack(true)}>Connect Slack</Button>
            <Button onClick={() => setConnectingTeams(true)}>Connect Teams</Button>
            <Button onClick={() => setConnectingChat(true)}>Connect Google Chat</Button>
            <Button onClick={() => setConnectingTelegram(true)}>Connect Telegram</Button>
            <Button variant="primary" onClick={() => setAdding(true)}>
              Add a file
            </Button>
          </div>
        }
      />

      {sources.isSuccess && sources.data.length === 0 ? (
        <EmptyState
          title="Start with messages you already have"
          body="Import a WhatsApp chat, Telegram export, Messenger or Instagram download, Android text message backup, Discord data package, email export (.mbox), or Mimic JSON file. No account connection is required. Only import conversations you own or have permission to process."
          primary={
            <div className="row gap-2">
              <Button variant="primary" onClick={() => setAdding(true)}>
                Add a file
              </Button>
              <Button onClick={() => setConnecting(true)}>Connect a mailbox (optional)</Button>
              <Button onClick={() => setConnectingSlack(true)}>Connect Slack (optional)</Button>
              <Button onClick={() => setConnectingTeams(true)}>Connect Teams (optional)</Button>
              <Button onClick={() => setConnectingChat(true)}>
                Connect Google Chat (optional)
              </Button>
              <Button onClick={() => setConnectingTelegram(true)}>
                Connect Telegram (optional)
              </Button>
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
                    <div className="muted small mono">{whereFrom(s)}</div>
                    {s.lastError ? (
                      <div className="danger small">{String(s.lastError.message)}</div>
                    ) : null}
                    {s.lastError?.transcript ? (
                      <ServerTranscript lines={s.lastError.transcript} />
                    ) : null}
                    {s.connector === "imap" ? (
                      <>
                        <MailboxFolders sourceId={s.id} name={s.name} />
                        <MailboxSending sourceId={s.id} name={s.name} />
                      </>
                    ) : null}
                    {s.connector === "google_chat" ? (
                      <GoogleChatSpaces
                        sourceId={s.id}
                        on={googleChatAccount(s)?.spaces ?? false}
                      />
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
                        {s.connector === "imap" ||
                        s.connector === "slack" ||
                        s.connector === "teams" ||
                        s.connector === "google_chat" ||
                        s.connector === "telegram_account"
                          ? "Check now"
                          : s.messageCount > 0
                            ? "Import again"
                            : "Import"}
                      </Button>
                      <Button size="sm" variant="ghost" onClick={() => remove.mutate(s.id)}>
                        Remove
                      </Button>
                    </div>
                    {mailboxAuth(s) === "microsoft" || mailboxAuth(s) === "google" ? (
                      <MailboxSignInAgain
                        sourceId={s.id}
                        name={s.name}
                        signInWith={mailboxAuth(s) === "google" ? "google" : "microsoft"}
                      />
                    ) : s.connector === "imap" ? (
                      <MailboxPassword sourceId={s.id} name={s.name} />
                    ) : s.connector === "slack" ? (
                      <SlackToken sourceId={s.id} name={s.name} />
                    ) : s.connector === "teams" ? (
                      <TeamsSignInAgain sourceId={s.id} name={s.name} />
                    ) : s.connector === "google_chat" ? (
                      <GoogleChatSignInAgain sourceId={s.id} name={s.name} />
                    ) : s.connector === "telegram_account" ? (
                      <Button size="sm" variant="ghost" onClick={() => setConnectingTelegram(true)}>
                        Sign in again
                      </Button>
                    ) : null}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          <p className="muted small">
            Importing the same file again adds only what&rsquo;s new: messages are known by the
            source they came from and their own id. Mail read by an earlier version gets what that
            version couldn&rsquo;t tell &mdash; whether it came from your Sent folder, and whether a
            machine sent it.
          </p>
        </Card>
      )}

      {hasMailbox ? <SentList /> : null}

      {adding ? <AddSourceDialog onClose={() => setAdding(false)} /> : null}
      {connecting ? <ConnectMailboxDialog onClose={() => setConnecting(false)} /> : null}
      {connectingSlack ? <ConnectSlackDialog onClose={() => setConnectingSlack(false)} /> : null}
      {connectingTeams ? <ConnectTeamsDialog onClose={() => setConnectingTeams(false)} /> : null}
      {connectingChat ? <ConnectGoogleChatDialog onClose={() => setConnectingChat(false)} /> : null}
      {connectingTelegram ? (
        <ConnectTelegramDialog onClose={() => setConnectingTelegram(false)} />
      ) : null}
    </div>
  );
}
