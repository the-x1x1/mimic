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
  /** `fileOrFolder`: a file, or the folder it unzips to (a Discord package). */
  locationKind: z.enum(["file", "folder", "fileOrFolder"]),
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

/** An address, as the import gives it and as the user's own are matched. */
export const WriterAddress = z.object({
  kind: z.string(),
  value: z.string(),
  /** What matching compares, as the user's own addresses store it. */
  normalized: z.string(),
});
export type WriterAddress = z.infer<typeof WriterAddress>;

/**
 * Someone who wrote in a chat that knows people only by name, as the import
 * will know them: saying which is the user adds exactly this address.
 */
export const WriterName = z.object({
  name: z.string(),
  /**
   * `handle` (`whatsapp:<name>`) or `phone`, for a writer shown as a number;
   * `account_id` (`discord:<id>`) for a Discord package's account.
   */
  kind: z.string(),
  value: z.string(),
  /** What matching compares, as the user's own addresses store it. */
  normalized: z.string(),
  messages: z.number(),
  /** A chat here is named after them: in one between two people, the one it is with. */
  chatNamedAfter: z.boolean(),
  /** Other addresses the import gives the same writer: a Discord account's email. */
  also: z.array(WriterAddress).default([]),
});
export type WriterName = z.infer<typeof WriterName>;

/**
 * Which of the user's addresses makes this writer the user, if any: the one
 * the import gives them first, then any other it gives them too (a Discord
 * account's email). The import decides by the same match.
 */
export function yoursAs<T extends { kind: string; normalizedValue: string }>(
  writer: WriterName,
  identifiers: readonly T[],
): T | null {
  for (const a of [writer, ...writer.also]) {
    const found = identifiers.find((i) => i.kind === a.kind && i.normalizedValue === a.normalized);
    if (found) return found;
  }
  return null;
}

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
  /** Messages read as from the user's Sent folder: Gmail's label, or the file's name. */
  sentFolder: z.number(),
  /**
   * For a source that knows people only by the name a phone saved them under
   * (WhatsApp): everyone who wrote, most messages first, so the user can say
   * which is theirs; for a Discord package, its account. Empty for any other
   * source.
   */
  names: z.array(WriterName).default([]),
  /**
   * Everything here is by the one writer in `names` (a Discord package), and
   * is imported only once that writer is the user.
   */
  oneWriter: z.boolean().default(false),
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
  /** Messages already here that reading them again marked as from the Sent folder. */
  sentFolderMarked: z.number().optional(),
  /** Messages already here that reading them again marked as sent by a machine. */
  automatedMarked: z.number().optional(),
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
  // What an earlier version couldn't tell about mail it had already read.
  const sent = s.sentFolderMarked ?? 0;
  const automated = s.automatedMarked ?? 0;
  if (sent > 0) parts.push(`${sent} already here now read as from your Sent folder`);
  if (automated > 0) parts.push(`${automated} already here now read as sent by a machine`);
  return parts.join(", ");
}

/** How a mailbox signs in: an app password, or a sign-in with Microsoft or Google. */
export const MailboxAuth = z.enum(["password", "microsoft", "google"]);
export type MailboxAuth = z.infer<typeof MailboxAuth>;

/** A provider a mailbox signs in with, in the browser. */
export type SignInWith = Exclude<MailboxAuth, "password">;

/** The provider's name, as the screen says it. */
export const SIGN_IN_NAMES: Record<SignInWith, string> = {
  microsoft: "Microsoft",
  google: "Google",
};

/** Which providers this copy of Mimic can sign in to a mailbox with. */
export const MailSignIns = z.object({ microsoft: z.boolean(), google: z.boolean() });
export type MailSignIns = z.infer<typeof MailSignIns>;

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
 * takes no password at all (`noPassword`): those sign in with Microsoft
 * (`signIn`), when this copy of Mimic was built able to. Gmail takes either:
 * a sign-in with Google when this copy can, or an app password.
 */
const MICROSOFT =
  "Outlook.com and Hotmail no longer let apps like this sign in with a password. You sign in with Microsoft in your browser instead.";

const OUTLOOK: KnownMailHost = {
  host: "outlook.office365.com",
  port: 993,
  help: MICROSOFT,
  signIn: "microsoft",
  noPassword: true,
};

/** Said when this copy of Mimic can't sign in with Microsoft. */
export const MICROSOFT_UNAVAILABLE =
  "Outlook.com and Hotmail no longer let apps like this sign in with a password, and this copy of Mimic isn't set up to sign in with Microsoft. For now, bring your mail in as an .mbox file: add the account to Thunderbird, then export the inbox and sent folders as mbox.";

export interface KnownMailHost {
  host: string;
  port: number;
  /** Where the provider makes an app password, or how it signs in. */
  help: string;
  /** The provider's own sign-in, which Mimic can use when built able to. */
  signIn?: SignInWith;
  /** It takes no password of any kind: only its sign-in. */
  noPassword?: true;
}

const ICLOUD: KnownMailHost = {
  host: "imap.mail.me.com",
  port: 993,
  help: "appleid.apple.com → Sign-In and Security → App-Specific Passwords.",
};

const YAHOO: KnownMailHost = {
  host: "imap.mail.yahoo.com",
  port: 993,
  help: "Yahoo Account security → Generate app password.",
};

const FASTMAIL: KnownMailHost = {
  host: "imap.fastmail.com",
  port: 993,
  help: "Fastmail → Settings → Privacy & Security → App passwords.",
};

const ZOHO: KnownMailHost = {
  host: "imap.zoho.com",
  port: 993,
  help: "Zoho Mail → Settings → Security → App passwords.",
};

/** Google Workspace at a domain of its own: signs in with Google, or takes an app password where the organisation allows them. */
const GOOGLE_WORKSPACE: KnownMailHost = {
  host: "imap.gmail.com",
  port: 993,
  help: "Google Account → Security → 2-Step Verification → App passwords, if your organisation allows them.",
  signIn: "google",
};

export const KNOWN_MAIL_HOSTS: Record<string, KnownMailHost> = {
  "gmail.com": {
    host: "imap.gmail.com",
    port: 993,
    help: "Google Account → Security → 2-Step Verification → App passwords.",
    signIn: "google",
  },
  "googlemail.com": {
    host: "imap.gmail.com",
    port: 993,
    help: "Google Account → Security → 2-Step Verification → App passwords.",
    signIn: "google",
  },
  // Microsoft stopped accepting passwords of any kind for IMAP on personal
  // accounts in September 2024; these sign in with Microsoft instead.
  "outlook.com": OUTLOOK,
  "hotmail.com": OUTLOOK,
  "live.com": OUTLOOK,
  "msn.com": OUTLOOK,
  "icloud.com": ICLOUD,
  "me.com": ICLOUD,
  "fastmail.com": FASTMAIL,
  "mac.com": ICLOUD,
  "yahoo.com": YAHOO,
  "ymail.com": YAHOO,
  "rocketmail.com": YAHOO,
  "yahoo.co.uk": YAHOO,
  "yahoo.fr": YAHOO,
  "yahoo.de": YAHOO,
  "yahoo.it": YAHOO,
  "yahoo.es": YAHOO,
  "yahoo.ca": YAHOO,
  "yahoo.com.au": YAHOO,
  "yahoo.in": YAHOO,
  "aol.com": {
    host: "imap.aol.com",
    port: 993,
    help: "AOL Account security → Generate app password.",
  },
  "zoho.com": ZOHO,
  "zohomail.com": ZOHO,
};

/** A domain's mail exchangers, most preferred first (desktop `mail_exchangers`). */
export const MailExchangers = z.array(z.string());

/** A provider found for an address at a domain of its own, and its name. */
export interface FoundMailHost extends KnownMailHost {
  /** Whose servers they are, as the dialog says it: "Google Workspace". */
  name: string;
}

/**
 * Whose mail servers a domain's mail exchangers (MX records, most preferred
 * first, as the desktop's `mail_exchangers` gives them) are, matched by the
 * exchanger's own domain and nothing looser: the first exchanger that is
 * one of these says. Null when none is.
 */
const BY_EXCHANGER: readonly (readonly [string, FoundMailHost])[] = [
  ["google.com", { name: "Google Workspace", ...GOOGLE_WORKSPACE }],
  ["googlemail.com", { name: "Google Workspace", ...GOOGLE_WORKSPACE }],
  [
    "mail.protection.outlook.com",
    {
      name: "Microsoft 365",
      host: "outlook.office365.com",
      port: 993,
      help: "Microsoft 365 doesn't let apps like this sign in with a password. You sign in with Microsoft in your browser instead.",
      signIn: "microsoft",
      noPassword: true,
    },
  ],
  ["zoho.com", { name: "Zoho Mail", ...ZOHO }],
  ["zoho.eu", { name: "Zoho Mail", ...ZOHO, host: "imap.zoho.eu" }],
  ["messagingengine.com", { name: "Fastmail", ...FASTMAIL }],
  ["mail.icloud.com", { name: "iCloud Mail", ...ICLOUD }],
  [
    "secureserver.net",
    {
      name: "GoDaddy",
      host: "imap.secureserver.net",
      port: 993,
      help: "GoDaddy's Workspace Email signs in with your email password.",
    },
  ],
  [
    "privateemail.com",
    {
      name: "Namecheap Private Email",
      host: "mail.privateemail.com",
      port: 993,
      help: "Namecheap's Private Email signs in with your mailbox password.",
    },
  ],
];

export function mailHostFromExchangers(exchangers: readonly string[]): FoundMailHost | null {
  for (const raw of exchangers) {
    const mx = raw.trim().toLowerCase().replace(/\.$/, "");
    const found = BY_EXCHANGER.find(([d]) => mx === d || mx.endsWith(`.${d}`));
    if (found) return found[1];
  }
  return null;
}

/** The domain of an address, lower-cased, or null when it has none yet. */
export function mailDomainOf(address: string): string | null {
  const domain = address.split("@")[1]?.trim().toLowerCase().replace(/\.$/, "");
  return domain && /^[a-z0-9-]+(\.[a-z0-9-]+)+$/.test(domain) ? domain : null;
}

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
export function guessMailHost(address: string): KnownMailHost | null {
  const domain = address.split("@")[1]?.trim().toLowerCase();
  if (!domain) return null;
  return (
    KNOWN_MAIL_HOSTS[domain] ??
    (MICROSOFT_ELSEWHERE.has(domain) ? (KNOWN_MAIL_HOSTS["outlook.com"] ?? null) : null)
  );
}
