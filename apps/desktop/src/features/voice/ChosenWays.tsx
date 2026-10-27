import type { ChosenWay, LearningOverview } from "@mimic/contracts";
import { countOf } from "@/lib/format";

/**
 * Which way of saying a draft the user chose over the first one, when other
 * ways were written beside it (0.10.0-alpha.97). A way chosen three times,
 * and more often than not, is how the next first draft is written; the rest
 * are listed as forming, with how far each has to go. Nothing is shown until
 * a choice has been made.
 */
export function ChosenWays({ overview }: { overview: LearningOverview }) {
  if (overview.choicesConsidered === 0) return null;
  const holding = overview.ways.filter((w) => w.holds);
  const forming = overview.ways.filter((w) => !w.holds);
  return (
    <section aria-label="Ways you chose">
      <h3 className="card-subhead">Ways you chose</h3>
      <p className="muted small">
        From {countOf(overview.choicesConsidered, "choice")} between ways of saying a draft.
      </p>
      {overview.ways.length === 0 ? (
        <p className="neutral">
          You sent my first draft over the other ways each time, so I&rsquo;ve kept writing it as I
          do.
        </p>
      ) : holding.length > 0 ? (
        <Ways ways={holding} />
      ) : (
        <p className="neutral">No way you&rsquo;ve chosen has added up to a habit yet.</p>
      )}
      {forming.length > 0 ? <Ways ways={forming} muted /> : null}
    </section>
  );
}

function Ways({ ways, muted = false }: { ways: ChosenWay[]; muted?: boolean }) {
  return (
    <ul className={muted ? "plain-list muted small" : "plain-list"}>
      {ways.map((w) => (
        <li key={`${w.adjustment}|${w.participantId ?? ""}`}>{w.summary}</li>
      ))}
    </ul>
  );
}
