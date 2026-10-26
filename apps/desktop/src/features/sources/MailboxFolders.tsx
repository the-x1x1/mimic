import { useState } from "react";
import { Button, InlineError } from "@mimic/ui";
import { useMailboxFolders, useSetMailboxFolders } from "@/hooks/useSources";

/**
 * Other folders a mailbox is read in (0.10.0-alpha.73): where the user's own
 * rules file mail away from the inbox. Looking signs in to the mailbox and
 * opens nothing; saving starts a check (even of a mailbox whose last one
 * failed), which reads what is chosen. Mail found there waits for the user —
 * it is never answered by itself.
 */
export function MailboxFolders({ sourceId, name }: { sourceId: string; name: string }) {
  const [open, setOpen] = useState(false);
  const [picked, setPicked] = useState<string[] | null>(null);
  const [saved, setSaved] = useState<number | null>(null);
  const folders = useMailboxFolders(sourceId, open);
  const save = useSetMailboxFolders(sourceId);

  if (!open) {
    return (
      <p className="muted small">
        {saved === null
          ? null
          : saved === 0
            ? "I read only your inbox and Sent folder now. "
            : `I'll read ${saved === 1 ? "that folder" : `those ${saved} folders`} too, from the check I've just started. `}
        <button
          type="button"
          className="linkish"
          onClick={() => {
            setPicked(null);
            save.reset();
            setOpen(true);
          }}
        >
          Other folders to read
        </button>
      </p>
    );
  }

  if (folders.isPending) {
    return (
      <p className="muted small" role="status">
        Looking at the folders in {name}&hellip;
      </p>
    );
  }

  if (folders.isError) {
    return (
      <div className="stack gap-1">
        <InlineError>{folders.error.message}</InlineError>
        <div className="row gap-1">
          <Button size="sm" variant="ghost" onClick={() => void folders.refetch()}>
            Try again
          </Button>
          <Button size="sm" variant="ghost" onClick={() => setOpen(false)}>
            Never mind
          </Button>
        </div>
      </div>
    );
  }

  const data = folders.data;
  const chosen = picked ?? data.folders.filter((f) => f.chosen).map((f) => f.name);
  const full = chosen.length >= data.most;

  return (
    <form
      className="stack gap-1"
      aria-label={`Other folders to read in ${name}`}
      onSubmit={(e) => {
        e.preventDefault();
        save.mutate(chosen, {
          onSuccess: (now) => {
            setSaved(now.folders.filter((f) => f.chosen).length);
            setOpen(false);
          },
        });
      }}
    >
      <p className="muted small">
        Besides your inbox and Sent folder, I can read the folders your own rules file mail into. I
        leave out drafts, deleted and junk mail where the server or the folder&rsquo;s name says
        that&rsquo;s what it holds. What I find there waits for you: I never reply to it by myself.
        The first time, I read the newest 500 messages in each.
      </p>
      {data.gone.length > 0 ? (
        <p className="muted small">
          {data.gone.length === 1 ? "This folder is" : "These folders are"} no longer in {name}, so
          I&rsquo;ve stopped reading {data.gone.length === 1 ? "it" : "them"}:{" "}
          {data.gone.join(", ")}.
        </p>
      ) : null}
      {data.unopened.length > 0 ? (
        <p className="muted small">
          The last check couldn&rsquo;t open {data.unopened.join(", ")}, so I passed{" "}
          {data.unopened.length === 1 ? "it" : "them"} over and read the rest.
        </p>
      ) : null}
      {data.folders.length === 0 ? (
        <p className="muted small">There are no other folders in {name} I can read.</p>
      ) : (
        <ul className="stack gap-1" aria-label="Folders">
          {data.folders.map((f) => {
            const on = chosen.includes(f.name);
            return (
              <li key={f.name}>
                <label className="row gap-1">
                  <input
                    type="checkbox"
                    checked={on}
                    disabled={save.isPending || (!on && full)}
                    onChange={(e) => {
                      save.reset();
                      setPicked(
                        e.target.checked ? [...chosen, f.name] : chosen.filter((n) => n !== f.name),
                      );
                    }}
                  />
                  <span>{f.shown}</span>
                </label>
              </li>
            );
          })}
        </ul>
      )}
      {full ? <p className="muted small">{data.most} folders at most.</p> : null}
      {save.error ? <InlineError>{save.error.message}</InlineError> : null}
      <div className="row gap-1">
        <Button size="sm" type="submit" disabled={save.isPending}>
          {save.isPending ? "Keeping…" : "Read these"}
        </Button>
        <Button size="sm" variant="ghost" type="button" onClick={() => setOpen(false)}>
          Never mind
        </Button>
      </div>
    </form>
  );
}
