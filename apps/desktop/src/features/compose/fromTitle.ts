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
