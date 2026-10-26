import { useState } from "react";
import { Button } from "@mimic/ui";

/**
 * What was said to a mail server when a check failed (0.10.0-alpha.38),
 * closed until asked for: Mimic's commands with any password or sign-in left
 * out, and the server's answers, never a message. It can be copied, to ask
 * for help with it.
 */
export function ServerTranscript({ lines }: { lines: readonly string[] }) {
  const [copied, setCopied] = useState(false);
  if (lines.length === 0) return null;
  const text = lines.join("\n");
  return (
    <details className="transcript">
      <summary className="muted small">What the server said</summary>
      <p className="muted small">
        Your password, your sign-in and your messages are never in this, only what was asked and
        answered.
      </p>
      <pre className="transcript__lines">{text}</pre>
      <Button
        size="sm"
        variant="ghost"
        onClick={() =>
          void navigator.clipboard.writeText(text).then(
            () => setCopied(true),
            () => setCopied(false),
          )
        }
      >
        {copied ? "Copied" : "Copy it"}
      </Button>
    </details>
  );
}
