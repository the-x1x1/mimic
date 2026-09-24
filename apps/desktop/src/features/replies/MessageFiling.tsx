import { useId, useState } from "react";
import { Button } from "@mimic/ui";
import { filedByPhrase, type Filing } from "@mimic/contracts";
import { useFileMessage, useSituations } from "@/hooks/useVoice";

/**
 * What one of the user's own messages is filed under as doing — saying no,
 * setting a time — and by whom, with a way to say otherwise. What they say
 * stands over the rules and any model until they hand it back.
 *
 * A message filed under nothing by the rules shows only the question, so a
 * conversation of ordinary messages is not lined with "doing none of these".
 */
export function MessageFiling({
  messageId,
  filing,
  excerpt,
}: {
  messageId: string;
  filing: Filing;
  /** The start of the message, to tell its controls from the next one's. */
  excerpt: string;
}) {
  const situations = useSituations();
  const file = useFileMessage();
  const [saved, setSaved] = useState<Filing | null>(null);
  const [editing, setEditing] = useState(false);
  const [chosen, setChosen] = useState<string[]>([]);
  const legend = useId();
  const filed = saved ?? filing;
  const vocabulary = situations.data ?? [];
  const labelOf = (id: string) => vocabulary.find((s) => s.id === id)?.label ?? id;
  const about = `“${excerpt}”`;

  const open = () => {
    setChosen(filed.situations);
    setEditing(true);
  };
  const save = (situationIds: string[] | null) =>
    file.mutate(
      { messageId, situationIds },
      {
        onSuccess: (now) => {
          setSaved(now);
          setEditing(false);
        },
      },
    );

  if (editing) {
    return (
      <fieldset className="stack gap-1 message-filing" aria-labelledby={legend}>
        <legend id={legend} className="small">
          What was this message doing? Tick none if it was none of these.
        </legend>
        <div className="row gap-2 wrap">
          {vocabulary.map((s) => (
            <label key={s.id} className="row gap-1 small">
              <input
                type="checkbox"
                checked={chosen.includes(s.id)}
                onChange={(e) =>
                  setChosen((c) =>
                    e.target.checked ? [...c, s.id] : c.filter((id) => id !== s.id),
                  )
                }
              />
              <span>{s.label}</span>
            </label>
          ))}
        </div>
        <div className="row gap-2">
          <Button
            size="sm"
            variant="primary"
            disabled={file.isPending || vocabulary.length === 0}
            onClick={() => save(vocabulary.map((s) => s.id).filter((id) => chosen.includes(id)))}
          >
            Save
          </Button>
          {filed.by !== "rules" ? (
            <Button size="sm" variant="ghost" disabled={file.isPending} onClick={() => save(null)}>
              Let the rules decide
            </Button>
          ) : null}
          <Button size="sm" variant="ghost" onClick={() => setEditing(false)}>
            Cancel
          </Button>
        </div>
      </fieldset>
    );
  }

  // The visible words first, then which message: the name a screen reader
  // gives it holds what the eye sees.
  const visible =
    filed.situations.length === 0 && filed.by === "rules" ? "What was this doing?" : "Change";
  const change = (
    <button type="button" className="linkish" aria-label={`${visible} ${about}`} onClick={open}>
      {visible}
    </button>
  );
  if (filed.situations.length === 0 && filed.by === "rules") {
    return <p className="muted small message-filing">{change}</p>;
  }
  const what =
    filed.situations.length === 0
      ? "Doing none of these"
      : filed.situations.map(labelOf).join(" · ");
  return (
    <p className="muted small message-filing">
      {what}, {filedByPhrase(filed.by)}. {change}
    </p>
  );
}
