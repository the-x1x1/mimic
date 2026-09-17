import { useEffect } from "react";
import { X } from "lucide-react";
import { useToasts } from "@/state/toast";

export function Toaster() {
  const { toasts, dismiss } = useToasts();
  useEffect(() => {
    if (toasts.length === 0) return;
    const t = window.setInterval(() => {
      const now = Date.now();
      for (const toast of toasts) if (now - toast.createdAt > 6000) dismiss(toast.id);
    }, 1000);
    return () => window.clearInterval(t);
  }, [toasts, dismiss]);
  return (
    <div className="toaster" aria-live="polite">
      {toasts.map((t) => (
        <div key={t.id} className={`toast toast--${t.tone}`} role="status">
          <div>
            <div className="toast__title">{t.title}</div>
            {t.body ? <div className="toast__body">{t.body}</div> : null}
          </div>
          <button className="toast__close" onClick={() => dismiss(t.id)} aria-label="Dismiss">
            <X size={14} />
          </button>
        </div>
      ))}
    </div>
  );
}
