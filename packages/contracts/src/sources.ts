import { z } from "zod";

export const Channel = z.enum(["email", "sms", "chat", "forum", "other"]);
export type Channel = z.infer<typeof Channel>;

export const CHANNEL_LABELS: Record<Channel, string> = {
  email: "Email",
  sms: "Text messages",
  chat: "Chat",
  forum: "Forums and threads",
  other: "Other",
};

export const ConnectorInfo = z.object({
  connector: z.string(),
  displayName: z.string(),
  channel: z.string(),
  description: z.string(),
  locationKind: z.enum(["file", "folder"]),
  extensions: z.array(z.string()),
});
export type ConnectorInfo = z.infer<typeof ConnectorInfo>;

export const SourceStatus = z.enum(["new", "ready", "importing", "imported", "failed"]);
export type SourceStatus = z.infer<typeof SourceStatus>;

export const Source = z.object({
  id: z.string(),
  connector: z.string(),
  name: z.string(),
  channel: z.string(),
  location: z.string().nullable(),
  config: z.record(z.string(), z.unknown()),
  status: z.string(),
  createdAt: z.string(),
  lastImportedAt: z.string().nullable(),
  messageCount: z.number(),
  lastError: z.object({ message: z.string() }).passthrough().nullable(),
});
export type Source = z.infer<typeof Source>;

/** What `validate` found, before the user commits to importing. */
export const ValidationReport = z.object({
  ok: z.boolean(),
  blockers: z.array(z.string()),
  warnings: z.array(z.string()),
  conversations: z.number(),
  messages: z.number(),
  frequentIdentifiers: z.array(z.tuple([z.string(), z.number()])),
  earliest: z.string().nullable(),
  latest: z.string().nullable(),
});
export type ValidationReport = z.infer<typeof ValidationReport>;

export const ImportSummary = z.object({
  conversations: z.number(),
  inserted: z.number(),
  duplicates: z.number(),
  empty: z.number(),
  fromSelf: z.number(),
  unattributed: z.number(),
  participantsCreated: z.number(),
});
export type ImportSummary = z.infer<typeof ImportSummary>;

/**
 * One line summarising an import, for the job tray. Says what was skipped and
 * why, because "45 messages imported" hides the 1 that could not be attributed.
 */
export function describeImport(s: ImportSummary): string {
  const parts = [`${s.inserted} messages from ${s.conversations} conversations`];
  if (s.fromSelf > 0) parts.push(`${s.fromSelf} written by you`);
  if (s.duplicates > 0) parts.push(`${s.duplicates} already imported`);
  if (s.unattributed > 0) parts.push(`${s.unattributed} with no identifiable author`);
  if (s.empty > 0) parts.push(`${s.empty} with no text`);
  return parts.join(", ");
}

/** How a mailbox signs in: an app password, or a sign-in with Microsoft. */
export const MailboxAuth = z.enum(["password", "microsoft"]);
export type MailboxAuth = z.infer<typeof MailboxAuth>;

/** How to reach a mailbox. The password is sent separately and never stored here. */
export const ImapAccount = z.object({
  host: z.string(),
  port: z.number(),
  username: z.string(),
  security: z.enum(["tls", "plain"]),
  /** Absent for a mailbox connected before 0.10.0-alpha.14: a password. */
  auth: MailboxAuth.optional(),
});
export type ImapAccount = z.infer<typeof ImapAccount>;

/** How a connected mailbox signs in, from its stored settings. */
export function mailboxAuth(source: {
  connector: string;
  config: Record<string, unknown>;
}): MailboxAuth | null {
  if (source.connector !== "imap") return null;
  const parsed = z
    .object({ account: z.object({ auth: MailboxAuth.optional() }) })
    .safeParse(source.config);
  return parsed.success ? (parsed.data.account.auth ?? "password") : "password";
}

/** What connecting found, before anything is read. */
export const ImapProbe = z.object({
  folders: z.array(z.string()),
  sentFolder: z.string().nullable(),
  counts: z.array(z.tuple([z.string(), z.number()])),
  warnings: z.array(z.string()),
});
export type ImapProbe = z.infer<typeof ImapProbe>;

/**
 * Server settings for the providers most people use. Most want an app
 * password for IMAP, not the account password; the dialog says so. Microsoft
 * takes no password at all: those sign in with Microsoft (`signIn`), when
 * this copy of Mimic was built able to.
 */
const MICROSOFT =
  "Outlook.com and Hotmail no longer let apps like this sign in with a password. You sign in with Microsoft in your browser instead.";

/** Said when this copy of Mimic can't sign in with Microsoft. */
export const MICROSOFT_UNAVAILABLE =
  "Outlook.com and Hotmail no longer let apps like this sign in with a password, and this copy of Mimic isn't set up to sign in with Microsoft. For now, bring your mail in as an .mbox file: add the account to Thunderbird, then export the inbox and sent folders as mbox.";

export const KNOWN_MAIL_HOSTS: Record<
  string,
  { host: string; port: number; help: string; signIn?: "microsoft" }
> = {
  "gmail.com": {
    host: "imap.gmail.com",
    port: 993,
    help: "Google Account → Security → 2-Step Verification → App passwords.",
  },
  "googlemail.com": {
    host: "imap.gmail.com",
    port: 993,
    help: "Google Account → Security → 2-Step Verification → App passwords.",
  },
  // Microsoft stopped accepting passwords of any kind for IMAP on personal
  // accounts in September 2024; these sign in with Microsoft instead.
  "outlook.com": { host: "outlook.office365.com", port: 993, help: MICROSOFT, signIn: "microsoft" },
  "hotmail.com": { host: "outlook.office365.com", port: 993, help: MICROSOFT, signIn: "microsoft" },
  "live.com": { host: "outlook.office365.com", port: 993, help: MICROSOFT, signIn: "microsoft" },
  "msn.com": { host: "outlook.office365.com", port: 993, help: MICROSOFT, signIn: "microsoft" },
  "icloud.com": {
    host: "imap.mail.me.com",
    port: 993,
    help: "appleid.apple.com → Sign-In and Security → App-Specific Passwords.",
  },
  "me.com": {
    host: "imap.mail.me.com",
    port: 993,
    help: "appleid.apple.com → Sign-In and Security → App-Specific Passwords.",
  },
  "fastmail.com": {
    host: "imap.fastmail.com",
    port: 993,
    help: "Fastmail → Settings → Privacy & Security → App passwords.",
  },
  "yahoo.com": {
    host: "imap.mail.yahoo.com",
    port: 993,
    help: "Yahoo Account security → Generate app password.",
  },
};

/**
 * Microsoft's own mail domains in other countries, which sign in as
 * outlook.com does. Named one by one rather than matched by shape: a shape
 * would take in live.io or outlook.co, which are other people's, and leave
 * someone there with no way to give a password.
 */
const MICROSOFT_ELSEWHERE = new Set(
  [
    "hotmail.co.uk hotmail.fr hotmail.de hotmail.it hotmail.es hotmail.be hotmail.ca hotmail.nl hotmail.se",
    "hotmail.co.jp hotmail.com.au hotmail.com.br hotmail.com.ar",
    "live.co.uk live.fr live.de live.it live.nl live.be live.ca live.at live.dk live.se live.no live.ie",
    "live.cl live.jp live.co.za live.com.au live.com.mx live.com.ar",
    "outlook.fr outlook.de outlook.it outlook.es outlook.be outlook.ie outlook.at outlook.dk outlook.pt",
    "outlook.cz outlook.sk outlook.hu outlook.cl outlook.jp outlook.kr outlook.sg outlook.ph outlook.my",
    "outlook.co.id outlook.co.il outlook.co.th outlook.com.au outlook.com.br outlook.com.ar outlook.com.gr",
    "outlook.com.tr outlook.com.vn windowslive.com passport.com",
  ].flatMap((line) => line.split(" ")),
);

/** Server settings guessed from an address, or null when the domain is unknown. */
export function guessMailHost(address: string) {
  const domain = address.split("@")[1]?.trim().toLowerCase();
  if (!domain) return null;
  return (
    KNOWN_MAIL_HOSTS[domain] ??
    (MICROSOFT_ELSEWHERE.has(domain) ? (KNOWN_MAIL_HOSTS["outlook.com"] ?? null) : null)
  );
}
