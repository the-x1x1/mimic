import type { Channel } from "@mimic/contracts";

/**
 * Who a message copied from anywhere is from, as the title of the window it
 * was copied in says (0.10.0-alpha.52): "Ada Lovelace | Messenger",
 * "@ada - Discord", "Chat | Ada Lovelace | Microsoft Teams". Only a person
 * named there whole, or by a first name no one else on the list has; when
 * two could be meant, nobody.
 */
export function personFromTitle(
  title: string | null | undefined,
  people: readonly { id: string; name: string }[],
): string | null {
  if (!title) return null;
  const says = (word: string) => {
    const w = word.trim().toLowerCase();
    if (w.length < 3) return false;
    const escaped = w.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    return new RegExp(`(?<![\\p{L}\\p{N}])${escaped}(?![\\p{L}\\p{N}])`, "u").test(
      title.toLowerCase(),
    );
  };
  const whole = people.filter((p) => says(p.name));
  if (whole.length === 1) return whole[0]?.id ?? null;
  if (whole.length > 1) return null;
  const first = (name: string) => name.trim().split(/\s+/)[0] ?? "";
  const byFirst = people.filter((p) => first(p.name).length >= 3 && says(first(p.name)));
  if (byFirst.length !== 1) return null;
  const only = byFirst[0];
  if (!only) return null;
  const shared = people.filter(
    (p) => first(p.name).toLowerCase() === first(only.name).toLowerCase(),
  );
  return shared.length === 1 ? only.id : null;
}

/**
 * The apps whose name a window's title ends with, or starts with, and what
 * kind of conversation a message copied from each is (0.10.0-alpha.83).
 * Matched as a whole part of the title, never a word inside one, so a mail
 * whose subject says "WhatsApp" in Gmail is still mail.
 */
const APPS: Readonly<Record<string, Channel>> = {
  whatsapp: "chat",
  "whatsapp web": "chat",
  discord: "chat",
  messenger: "chat",
  instagram: "chat",
  telegram: "chat",
  "telegram web": "chat",
  signal: "chat",
  slack: "chat",
  "microsoft teams": "chat",
  teams: "chat",
  "google chat": "chat",
  skype: "chat",
  viber: "chat",
  wechat: "chat",
  snapchat: "chat",
  element: "chat",
  "phone link": "sms",
  "your phone": "sms",
  "google messages": "sms",
  "messages for web": "sms",
  "google messages for web": "sms",
  outlook: "email",
  "microsoft outlook": "email",
  "outlook (new)": "email",
  gmail: "email",
  "mozilla thunderbird": "email",
  thunderbird: "email",
  mail: "email",
  "proton mail": "email",
  "yahoo mail": "email",
  "aol mail": "email",
  "icloud mail": "email",
  "em client": "email",
  mailbird: "email",
  "zoho mail": "email",
  spark: "email",
  // Classic Outlook's window for one message: "Subject - Message (HTML)".
  "message (html)": "email",
  "message (plain text)": "email",
  "message (rich text)": "email",
};

/**
 * What kind of conversation a message copied from anywhere came from, by the
 * title of its window (0.10.0-alpha.83): "Ada Lovelace | Messenger" a chat,
 * "Phone Link" a text, "Inbox - ada@x.com - Outlook" an email. The title is
 * cut at its separators (" - ", " | ", " • " and the like) and read from the
 * end, where an app or a browser page puts its name; null when no part is
 * one of them.
 */
export function channelFromTitle(title: string | null | undefined): Channel | null {
  if (!title) return null;
  // Edge puts a zero-width space in its own name; other apps use them too.
  const clean = title.replace(/[\u200b-\u200d\u2060\ufeff]/g, "");
  const parts = clean
    .split(/\s[-\u2013\u2014|\u2022\u00b7]\s/)
    .map((p) =>
      p
        .trim()
        .toLowerCase()
        .replace(/^\(\d+\)\s*/, ""),
    )
    .filter((p) => p.length > 0);
  for (let i = parts.length - 1; i >= 0; i -= 1) {
    const found = APPS[parts[i] ?? ""];
    if (found) return found;
  }
  return null;
}
