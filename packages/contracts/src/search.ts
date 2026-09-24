import { z } from "zod";
import { ThreadMessage } from "./app";

/**
 * Finding what was said: every message Mimic has read, searched by the words
 * in it, newest first (`Db::search_messages`).
 */

/**
 * Whose messages a search looks through: `others` is every message that is
 * not the user's, those whose writer could not be told included.
 */
export const SearchWho = z.enum(["anyone", "you", "others"]);
export type SearchWho = z.infer<typeof SearchWho>;

/** Part of the words around a match: a word that matched, or what is between. */
export const SnippetPiece = z.object({
  text: z.string(),
  hit: z.boolean(),
});
export type SnippetPiece = z.infer<typeof SnippetPiece>;

/** One message that says what was searched for. */
export const FoundMessage = z.object({
  message: ThreadMessage,
  conversationId: z.string(),
  subject: z.string().nullable(),
  /** The words around what matched, with the matches marked. */
  snippet: z.array(SnippetPiece),
  /** How many messages of its conversation come before it, and after. */
  earlier: z.number().int(),
  later: z.number().int(),
});
export type FoundMessage = z.infer<typeof FoundMessage>;

/** A page of what a search found, newest first. */
export const SearchPage = z.object({
  found: z.array(FoundMessage),
  /** Whether more were found past the last one here. */
  more: z.boolean(),
  /** How many messages say it in all, counted up to `SEARCH_COUNT_CAP`. */
  total: z.number().int(),
  /** There are more than `SEARCH_COUNT_CAP`. */
  capped: z.boolean(),
});
export type SearchPage = z.infer<typeof SearchPage>;

/** How far the matches of one search are counted (`search::COUNT_CAP`). */
export const SEARCH_COUNT_CAP = 1000;

/**
 * Whether what was typed holds anything to look for — a letter or a digit —
 * by the test the search itself makes (Rust's `char::is_alphanumeric`: the
 * Alphabetic property, or a number), so the screen never reports on a search
 * that was never made.
 */
export function hasWordsToFind(input: string): boolean {
  return /[\p{Alphabetic}\p{N}]/u.test(input);
}

/** How much a search found, as one line. */
export function describeFound(page: Pick<SearchPage, "total" | "capped">, who: SearchWho): string {
  if (page.total === 0) {
    if (who === "you") return "Nothing you wrote says that.";
    if (who === "others") return "Nothing anyone else wrote says that.";
    return "Nothing I've read says that.";
  }
  if (page.total === 1 && !page.capped) {
    if (who === "you") return "One of your messages says that.";
    if (who === "others") return "One message someone else wrote says that.";
    return "One message says that.";
  }
  const n = page.capped
    ? `More than ${SEARCH_COUNT_CAP.toLocaleString()}`
    : page.total.toLocaleString();
  const which =
    who === "you"
      ? `${n} of your messages`
      : who === "others"
        ? `${n} messages someone else wrote`
        : `${n} messages`;
  return page.capped
    ? `${which} say that. The newest come first; another word finds fewer.`
    : `${which} say that, the newest first.`;
}
