import { useState } from "react";
import { Card, InlineError } from "@mimic/ui";
import { ANYWHERE_SHORTCUTS } from "@mimic/contracts";
import { useSettings } from "@/hooks/useSystem";
import { ipc } from "@/lib/ipc";

/**
 * Answering a message from anywhere: copy it in any app, press the shortcut,
 * and Write opens on it. Off until a shortcut is chosen. One another app
 * already has is refused by Windows, and said so here; the one before stays.
 */
export function AnywhereSection() {
  const settings = useSettings();
  const chosen = settings.data?.["anywhere.shortcut"] ?? "";
  const pressEnter = settings.data?.["anywhere.pressEnter"] === true;
  const draftAtOnce = settings.data?.["anywhere.draftAtOnce"] !== false;
  const copyForMe = settings.data?.["anywhere.copyForMe"] === true;
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  async function choose(next: string) {
    setError(null);
    setSaving(true);
    try {
      await ipc.setSetting("anywhere.shortcut", next);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setSaving(false);
    }
  }

  return (
    <Card title="Answering a message from anywhere">
      <p className="neutral">
        WhatsApp, Discord, Messages on your phone and most other places don&rsquo;t let an app read
        or send for you. They all let you copy and paste. Copy a message there, press the shortcut,
        and I open Write on it &mdash; written to whoever the window&rsquo;s title names, when
        that&rsquo;s one person you write to. Then <strong>Paste it there</strong> puts my reply
        back where the message was, for you to send.
      </p>
      <label className="row gap-2">
        <span>Shortcut</span>
        <select value={chosen} disabled={saving} onChange={(e) => void choose(e.target.value)}>
          <option value="">Off</option>
          {ANYWHERE_SHORTCUTS.map((s) => (
            <option key={s} value={s}>
              {s}
            </option>
          ))}
        </select>
      </label>
      <label className="row gap-2">
        <input
          type="checkbox"
          checked={copyForMe}
          onChange={(e) => {
            setError(null);
            ipc
              .setSetting("anywhere.copyForMe", e.target.checked)
              .catch((err: unknown) => setError((err as Error).message));
          }}
        />
        <span>Copy what I&rsquo;ve selected when I press the shortcut</span>
      </label>
      {copyForMe ? (
        <p className="muted small">
          Select the message and press the shortcut. Once you&rsquo;ve let go of its keys, I press
          Ctrl+Insert in that window &mdash; copy, in every app, and never Ctrl+C, which stops what
          a terminal is running. If nothing is selected, I use what you copied last.
        </p>
      ) : null}
      <label className="row gap-2">
        <input
          type="checkbox"
          checked={draftAtOnce}
          onChange={(e) => {
            setError(null);
            ipc
              .setSetting("anywhere.draftAtOnce", e.target.checked)
              .catch((err: unknown) => setError((err as Error).message));
          }}
        />
        <span>Write a draft as soon as it opens, with the keyboard on putting it back</span>
      </label>
      <label className="row gap-2">
        <input
          type="checkbox"
          checked={pressEnter}
          onChange={(e) => {
            setError(null);
            ipc
              .setSetting("anywhere.pressEnter", e.target.checked)
              .catch((err: unknown) => setError((err as Error).message));
          }}
        />
        <span>Offer Paste it there and send, which presses Enter after pasting</span>
      </label>
      <p className="muted small">
        For chat apps where Enter sends &mdash; WhatsApp, Discord, Slack, Teams, Messenger. I press
        Enter once, in the window the message came from, and only if it&rsquo;s still in front and
        its title still says what it did when you copied the message. WhatsApp&rsquo;s title is the
        same for every chat, so stay in that chat until it&rsquo;s sent. In an email or a document
        Enter only starts a new line, so use Paste it there for those.
      </p>
      <p className="muted small">
        I read what you copied, and the title of the window you were in, only when you press it, and
        they go into Write and nowhere else until you ask for a draft. Paste it there presses Ctrl+V
        in that window and nothing else; Paste it there and send, Ctrl+V and then Enter. It works
        while I&rsquo;m running, in the notification area too.
      </p>
      {error ? <InlineError>{error}</InlineError> : null}
    </Card>
  );
}
