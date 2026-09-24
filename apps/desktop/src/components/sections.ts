/**
 * The drawer's sections: everything that is not answering mail, in the order
 * the bar and the drawer give them. Kept apart from the shell so the shell's
 * file exports components only.
 */
export const SECTIONS = [
  { to: "/sources", label: "Your mail" },
  { to: "/people", label: "People" },
  { to: "/voice", label: "How you write" },
  { to: "/settings", label: "Settings" },
] as const;

/** The section a path opens, or null for one that is not a section (Write). */
export function sectionOf(pathname: string): string | null {
  return SECTIONS.find((s) => pathname === s.to || pathname.startsWith(`${s.to}/`))?.label ?? null;
}
