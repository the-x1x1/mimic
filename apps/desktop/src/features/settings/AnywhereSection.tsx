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
        and I open Write on it, ready for a reply in your voice to copy back.
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
      <p className="muted small">
        I read what you copied only when you press it, and it goes into Write and nowhere else until
        you ask for a draft. It works while I&rsquo;m running, in the notification area too.
      </p>
      {error ? <InlineError>{error}</InlineError> : null}
    </Card>
  );
}
