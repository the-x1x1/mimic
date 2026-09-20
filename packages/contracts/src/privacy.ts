import { z } from "zod";

/** What a deletion did, or would do. Shown before the user confirms. */
export const DeletionReport = z.object({
  participants: z.number(),
  identifiers: z.number(),
  conversations: z.number(),
  messages: z.number(),
  ownMessages: z.number(),
  embeddings: z.number(),
  representativeExamples: z.number(),
  voiceProfiles: z.number(),
  voicePreferences: z.number(),
  drafts: z.number(),
  profilesInvalidated: z.number(),
  conversationsKept: z.number(),
});
export type DeletionReport = z.infer<typeof DeletionReport>;

/**
 * The deletion described in sentences, so the confirmation dialog states the
 * consequences rather than showing a table of counts. The user's own messages
 * are called out separately: people do not expect "delete Ada" to delete what
 * they themselves wrote to her, and it does.
 */
export function describeDeletion(r: DeletionReport): string[] {
  const lines: string[] = [];
  if (r.messages > 0) {
    lines.push(`${r.messages} messages will be deleted, across ${r.conversations} conversations.`);
  }
  if (r.ownMessages > 0) {
    lines.push(
      `${r.ownMessages} of those are messages you wrote. A conversation cannot be half-deleted, so your side of it goes too.`,
    );
  }
  if (r.conversationsKept > 0) {
    lines.push(
      `${r.conversationsKept} group conversations will be kept, with this person's messages removed from them.`,
    );
  }
  if (r.voiceProfiles > 0 || r.representativeExamples > 0) {
    lines.push(
      `Their voice profile, ${r.representativeExamples} example messages and ${r.voicePreferences} preferences you set for them will be removed.`,
    );
  }
  if (r.drafts > 0) {
    lines.push(`${r.drafts} drafts written to them will be removed.`);
  }
  if (r.profilesInvalidated > 0) {
    lines.push(
      `${r.profilesInvalidated} other profiles were built partly from this material and will need recomputing.`,
    );
  }
  lines.push("This cannot be undone.");
  return lines;
}
