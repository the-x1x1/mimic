import { z } from "zod";

export const ProviderInfo = z.object({
  id: z.string(),
  displayName: z.string(),
  /** True when nothing leaves this machine. The UI must show this. */
  local: z.boolean(),
  model: z.string(),
  description: z.string(),
  requiresCredential: z.boolean(),
});
export type ProviderInfo = z.infer<typeof ProviderInfo>;

/**
 * How this build protects saved credentials — an API key, mailbox app
 * passwords. `account`: sealed to the user's Windows account (DPAPI), so a
 * copy of the file cannot be opened without their Windows password, though a
 * program running as them, or an administrator, can still unseal it.
 * `keychain`: sealed with a key kept in the user's login Keychain (macOS; not
 * yet run on a Mac). `file`: in a file only this user can open, not sealed.
 * The store reports it; nothing assumes it.
 */
export const CredentialProtection = z.enum(["account", "keychain", "file"]);
export type CredentialProtection = z.infer<typeof CredentialProtection>;

export const CredentialState = z.object({
  protection: CredentialProtection,
  /**
   * On a build that seals: something unsealed is still on disk — the old
   * file, whether or not what was in it has been moved in.
   */
  unsealedLeft: z.boolean(),
  /**
   * Keys saved but not openable here — sealed to another Windows account or
   * computer, or before the account's password was reset. Kept, not deleted.
   * Never values.
   */
  locked: z.array(z.string()),
  /**
   * The file they are kept in could not be read: `setAside` (renamed, and
   * saves start a new one) or `leftAlone` (nothing is saved over it). Said in
   * the session it happens, and logged.
   */
  unreadable: z.enum(["setAside", "leftAlone"]).nullable(),
});
export type CredentialState = z.infer<typeof CredentialState>;

export const ProviderState = z.object({
  providers: z.array(ProviderInfo),
  active: z.string().nullable(),
  /** Which credential keys have a value that opens here. Never the values. */
  configuredSecrets: z.array(z.string()),
  credentials: CredentialState,
});
export type ProviderState = z.infer<typeof ProviderState>;

/**
 * What the Privacy card says about saved keys and passwords: what protects
 * them, what does not, and anything that needs doing.
 */
export function describeCredentials(c: CredentialState): string {
  const parts: string[] = [];
  if (c.protection === "account") {
    parts.push(
      "Any API key or mailbox password you give me is locked to your Windows account, so a copy of the file it's kept in can't be opened without your Windows password. A program you run yourself could still unlock it, and so could an administrator of this computer or, on a work account, your organisation's IT — the same as your browser's saved passwords.",
    );
    if (c.unsealedLeft) {
      parts.push(
        "The file they were kept in before this version isn't locked and is still on this computer; I'll move what's in it and delete it as soon as I can.",
      );
    }
  } else if (c.protection === "keychain") {
    parts.push(
      "Any API key or mailbox password you give me is locked with a key kept in your login Keychain, so a copy of the file it's kept in can't be opened without it. A program you run yourself could still ask for that key — the same as your browser's saved passwords — though macOS may ask you first.",
    );
    if (c.unsealedLeft) {
      // Saved by a build that did not seal: those values are not sealed
      // after the fact, so the promise is only what entering them does.
      parts.push(
        "Something saved before this version is still on this computer unlocked; entering those keys and passwords again locks them.",
      );
    }
  } else {
    parts.push(
      "Any API key or mailbox password you give me is kept in a file only your account can open. It isn't locked to your account, so anything that can read that file can read it.",
    );
  }
  if (c.unreadable === "setAside") {
    parts.push(
      "When I started, I couldn't read the file your saved keys and passwords were in, so I put it aside; any you saved before then need entering again.",
    );
  } else if (c.unreadable === "leftAlone") {
    parts.push(
      "I can't read the file your saved keys and passwords are in, so I've left it as it is and won't save over it. A newer version of Mimic may have written it.",
    );
  }
  const n = c.locked.length;
  if (n > 0) {
    const one = n === 1;
    parts.push(
      c.protection === "keychain"
        ? `I can't unlock ${one ? "one saved key or password" : `${n} saved keys or passwords`} any more: ${one ? "it was" : "they were"} locked with a key I can't get from your Keychain — it isn't there, or I wasn't allowed it — so ${one ? "it needs" : "they need"} entering again.`
        : `I can't unlock ${one ? "one saved key or password" : `${n} saved keys or passwords`} any more: ${one ? "it was" : "they were"} locked to another Windows account or computer, or before your Windows password was reset, so ${one ? "it needs" : "they need"} entering again.`,
    );
  }
  return parts.join(" ");
}

export const ANTHROPIC_SECRET_KEY = "provider.anthropic.apiKey";

/**
 * What is true about the model on this computer, and the one thing to do next.
 *
 * The step is computed in the core and sent here rather than being worked out
 * on screen, so the UI cannot invent a fourth situation or disagree with the
 * command that acts on it.
 */
export const LocalModelStatus = z.object({
  endpoint: z.string(),
  hostReachable: z.boolean(),
  models: z.array(z.string()),
  wanted: z.string(),
  wantedPresent: z.boolean(),
  error: z.string().nullable(),
  nextStep: z.enum(["ready", "getTheHost", "pullModel"]),
  downloadPage: z.string(),
});
export type LocalModelStatus = z.infer<typeof LocalModelStatus>;
