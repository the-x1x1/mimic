import { useEffect, useRef, useState } from "react";
import { Badge, Button, InlineError } from "@mimic/ui";
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
import { ConnectSlackDialog, SlackChannels, SlackToken } from "./ConnectSlackDialog";
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

/** Connectors that are read again on their own, rather than imported once. */
const LIVE = new Set(["imap", "slack", "teams", "google_chat", "telegram_account"]);

type WayIn = "file" | "mailbox" | "slack" | "teams" | "chat" | "telegram";

/**
 * The ways in, each saying what it reads, so choosing one needs no guess. A
 * file comes first: it is the one that needs no account and no sign-in.
 */
const WAYS_IN: { id: WayIn; name: string; note: string }[] = [
  {
    id: "file",
    name: "Add a file",
    note: "An export you already have: mail (.mbox), WhatsApp, Telegram, Messenger, Instagram, texts, Discord.",
  },
  {
    id: "mailbox",
    name: "Connect a mailbox",
    note: "Gmail, Outlook, iCloud or any IMAP mailbox. I check it for new mail and can send from it.",
  },
  {
    id: "slack",
    name: "Connect Slack",
    note: "Your direct and group messages, with your own token.",
  },
  { id: "teams", name: "Connect Teams", note: "Your chats, through a work or school account." },
  {
    id: "chat",
    name: "Connect Google Chat",
    note: "Direct messages and group chats in a Google Workspace account.",
  },
  { id: "telegram", name: "Connect Telegram", note: "Your chats, signed in as you." },
];

export function SourcesPage() {
  const sources = useSources();
  const [open, setOpen] = useState<WayIn | null>(null);
  const list = sources.data ?? [];
  const hasMailbox = list.some((s) => s.connector === "imap");
  const close = () => setOpen(null);

  return (
    <div className="stack gap-5">
      <PageHeader
        title="Your mail"
        subtitle={
          hasMailbox
            ? "Everything I know comes from mail you gave me: files you handed over, and the mailbox you connected, which I check for new mail. Nothing is uploaded. A reply you send from me goes out through that mailbox, as if you'd sent it yourself."
            : "Everything I know comes from files you hand me. Nothing is uploaded, and nothing arrives on its own."
        }
      />

      {sources.isError ? <InlineError>{(sources.error as Error).message}</InlineError> : null}

      {list.length > 0 ? (
        <section className="stack gap-2" aria-labelledby="sources-title">
          <h2 id="sources-title" className="section-title">
            Where your messages come from
          </h2>
          <ul className="sources">
            {list.map((s) => (
              <SourceRow key={s.id} source={s} onTelegram={() => setOpen("telegram")} />
            ))}
          </ul>
          <p className="muted small">
            Importing the same file again adds only what&rsquo;s new: messages are known by the
            source they came from and their own id. Mail read by an earlier version gets what that
            version couldn&rsquo;t tell &mdash; whether it came from your Sent folder, and whether a
            machine sent it.
          </p>
        </section>
      ) : null}

      <section className="stack gap-3" aria-labelledby="ways-in-title">
        <div className="stack gap-1">
          <h2 id="ways-in-title" className="section-title">
            {sources.isSuccess && list.length === 0
              ? "Start with messages you already have"
              : "Bring in more"}
          </h2>
          {sources.isSuccess && list.length === 0 ? (
            <p className="muted small">
              No account is needed: a file you already have is enough. Only bring in conversations
              you own or have permission to read.
            </p>
          ) : null}
        </div>
        <div className="ways-in">
          {WAYS_IN.map((w) => (
            <button
              key={w.id}
              type="button"
              className={w.id === "file" ? "way-in way-in--first" : "way-in"}
              aria-labelledby={`way-in-${w.id}-name`}
              aria-describedby={`way-in-${w.id}-note`}
              onClick={() => setOpen(w.id)}
            >
              <span id={`way-in-${w.id}-name`} className="way-in__name">
                {w.name}
              </span>
              <span id={`way-in-${w.id}-note`} className="way-in__note">
                {w.note}
              </span>
            </button>
          ))}
        </div>
      </section>

      {hasMailbox ? <SentList /> : null}

      {open === "file" ? <AddSourceDialog onClose={close} /> : null}
      {open === "mailbox" ? <ConnectMailboxDialog onClose={close} /> : null}
      {open === "slack" ? <ConnectSlackDialog onClose={close} /> : null}
      {open === "teams" ? <ConnectTeamsDialog onClose={close} /> : null}
      {open === "chat" ? <ConnectGoogleChatDialog onClose={close} /> : null}
      {open === "telegram" ? <ConnectTelegramDialog onClose={close} /> : null}
    </div>
  );
}

/**
 * One place messages come from: what it is, where from, how it stands, and
 * what can be done with it. A row of its own rather than a table's: a table
 * of six columns was wider than the drawer it sits in.
 */
function SourceRow({ source: s, onTelegram }: { source: Source; onTelegram: () => void }) {
  const startImport = useStartImport();
  const remove = useDeleteSource();
  const [confirming, setConfirming] = useState(false);
  const removeButton = useRef<HTMLButtonElement>(null);
  const keepButton = useRef<HTMLButtonElement>(null);
  const live = LIVE.has(s.connector);
  // Asking takes focus to the safe answer; keeping it gives focus back to Remove.
  const asked = useRef(false);
  useEffect(() => {
    if (confirming) keepButton.current?.focus();
    else if (asked.current) removeButton.current?.focus();
    asked.current = confirming;
  }, [confirming]);
  const confirmId = `remove-${s.id}`;
  const messages =
    s.messageCount === 1 ? "1 message" : `${s.messageCount.toLocaleString()} messages`;

  return (
    <li className="source">
      <div className="source__top">
        <div className="source__what">
          <div className="source__name">{s.name}</div>
          <div className="muted small mono source__where">{whereFrom(s)}</div>
        </div>
        <div className="source__actions">
          <Button
            size="sm"
            onClick={() => startImport.mutate(s.id)}
            disabled={s.status === "importing" || startImport.isPending}
          >
            {live ? "Check now" : s.messageCount > 0 ? "Import again" : "Import"}
          </Button>
          <Button
            size="sm"
            variant="ghost"
            ref={removeButton}
            aria-expanded={confirming}
            aria-controls={confirming ? confirmId : undefined}
            disabled={confirming}
            onClick={() => setConfirming(true)}
          >
            Remove
          </Button>
        </div>
      </div>
      <div className="source__facts small">
        <Badge tone={STATUS_TONE[s.status] ?? "neutral"}>{s.status}</Badge>
        <span>{CHANNEL_LABELS[s.channel as Channel] ?? s.channel}</span>
        <span>{messages}</span>
        {/* A live source says when it was checked where it says where it's read from. */}
        {live ? null : (
          <span className="muted">
            {s.lastImportedAt ? `imported ${formatRelative(s.lastImportedAt)}` : "not imported yet"}
          </span>
        )}
      </div>

      {confirming ? (
        <div
          id={confirmId}
          className="source__confirm"
          role="group"
          aria-label={`Remove ${s.name}`}
        >
          {/* What delete_source_and_contents and delete_source do: the
              messages, anyone known only from them, the record of what was
              sent through it, the sign-in; how you write is measured again. */}
          <p>
            Remove {s.name}? The {messages} I read from it go too, and so does anyone I know only
            from it{live ? ", the record of replies sent through it, and how I sign in to it" : ""}.
            How you write is worked out again without them. This can&rsquo;t be undone.
            {live ? null : " The file itself isn’t touched, so you can import it again."}
          </p>
          {remove.isError ? <InlineError>{(remove.error as Error).message}</InlineError> : null}
          <div className="row gap-2">
            <Button
              size="sm"
              variant="danger"
              disabled={remove.isPending}
              onClick={() => remove.mutate(s.id)}
            >
              Remove it
            </Button>
            <Button ref={keepButton} size="sm" variant="ghost" onClick={() => setConfirming(false)}>
              Keep it
            </Button>
          </div>
        </div>
      ) : null}

      {s.lastError ? <div className="danger small">{String(s.lastError.message)}</div> : null}
      {s.lastError?.transcript ? <ServerTranscript lines={s.lastError.transcript} /> : null}
      {s.connector === "imap" ? (
        <>
          <MailboxFolders sourceId={s.id} name={s.name} />
          <MailboxSending sourceId={s.id} name={s.name} />
        </>
      ) : null}
      {s.connector === "slack" ? (
        <SlackChannels
          sourceId={s.id}
          on={slackWorkspace(s)?.channels ?? false}
          missing={slackWorkspace(s)?.channelsMissing ?? null}
        />
      ) : null}
      {s.connector === "google_chat" ? (
        <GoogleChatSpaces sourceId={s.id} on={googleChatAccount(s)?.spaces ?? false} />
      ) : null}
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
        <div>
          <Button size="sm" variant="ghost" onClick={onTelegram}>
            Sign in again
          </Button>
        </div>
      ) : null}
    </li>
  );
}
