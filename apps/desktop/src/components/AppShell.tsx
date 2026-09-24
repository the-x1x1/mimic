import { useEffect, useRef } from "react";
import { NavLink, Outlet, useLocation, useNavigate } from "react-router-dom";
import { useSystemStatus } from "@/hooks/useSystem";
import { useProviderHealth, useProviderState } from "@/hooks/useCompose";
import { EngineBadge, ProviderBadge, UpdateBadge } from "./StatusBadges";
import { JobTray } from "./JobTray";
import { Toaster } from "./Toaster";
import { RepliesPage } from "@/features/replies/RepliesPage";
import { SECTIONS, sectionOf } from "./sections";

/**
 * There is one screen, and it is the replies. Everything else — people, how
 * you write, where your mail came from, settings — opens in a drawer over it
 * and closes back onto it.
 *
 * That is why the child routes render into a panel here rather than replacing
 * the page: a five-item sidebar asks someone to decide where to go before they
 * have seen anything, and the answer is always "the replies". Keeping the
 * routes means every existing link and deep link still works; they just land
 * on the drawer instead of a page of their own.
 *
 * The way in is a short row of words in the bar — Your mail, People, How you
 * write, Settings — and the same row at the top of the drawer, so one section
 * leads to the next without going back. (From 0.9.0-alpha.2 to 0.10.0-alpha.16
 * the bar had only Settings, and People and How you write could not be
 * reached at all.)
 */

function Sections({ label, className }: { label: string; className: string }) {
  return (
    <nav aria-label={label} className={className}>
      {SECTIONS.map((s) => (
        <NavLink
          key={s.to}
          to={s.to}
          className={({ isActive }) =>
            isActive ? "section-link section-link--on" : "section-link"
          }
        >
          {s.label}
        </NavLink>
      ))}
    </nav>
  );
}
export function AppShell() {
  const location = useLocation();
  const navigate = useNavigate();
  const open = location.pathname !== "/";

  const drawer = useRef<HTMLElement>(null);
  const returnTo = useRef<HTMLElement | null>(null);
  const wasOpen = useRef(open);
  // What had focus when the drawer opened, read while rendering — before
  // anything in the drawer (Compose's box, say) can take focus for itself.
  if (open && !wasOpen.current) {
    const active = document.activeElement;
    returnTo.current = active instanceof HTMLElement ? active : null;
  }
  wasOpen.current = open;

  // Escape closes the drawer, because a panel that can only be dismissed with
  // a small × is a panel someone gets stuck in — unless a dialog in the drawer
  // took it to close itself, which marks it handled.
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !e.defaultPrevented) navigate("/");
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, navigate]);

  // Opening the drawer takes focus into it (unless something in it already
  // has it), and closing it gives focus back to what opened it, so the keys
  // follow what is on screen.
  useEffect(() => {
    if (open) {
      if (drawer.current && !drawer.current.contains(document.activeElement)) {
        drawer.current.focus();
      }
    } else if (returnTo.current) {
      returnTo.current.focus();
      returnTo.current = null;
    }
  }, [open]);

  return (
    <div className="app">
      <TopBar />
      <main className="content">
        <div className="content__inner">
          <RepliesPage />
        </div>
      </main>
      {open ? (
        <>
          <button
            type="button"
            className="drawer__scrim"
            aria-label="Close"
            onClick={() => navigate("/")}
          />
          <aside
            ref={drawer}
            tabIndex={-1}
            className="drawer"
            role="dialog"
            aria-modal="true"
            aria-label={sectionOf(location.pathname) ?? "Write something new"}
          >
            <div className="drawer__head">
              <button
                type="button"
                className="drawer__close"
                aria-label="Close and go back to your replies"
                onClick={() => navigate("/")}
              >
                Back to replies
              </button>
              <Sections label="Sections" className="drawer__nav" />
            </div>
            <div className="drawer__body">
              <Outlet />
            </div>
          </aside>
        </>
      ) : null}
      <Toaster />
    </div>
  );
}

/**
 * The whole of the navigation. A wordmark, whatever is running, and the way
 * into everything else.
 */
function TopBar() {
  const system = useSystemStatus();
  const providers = useProviderState();
  const navigate = useNavigate();
  const active = providers.data?.providers.find((p) => p.id === providers.data?.active);
  const health = useProviderHealth(active?.id);
  return (
    <header className="topbar">
      <div className="topbar__brand">
        <span className="brand__mark" aria-hidden />
        <span className="topbar__name">Mimic</span>
        <span className="topbar__tag">writing as you, on this computer</span>
      </div>
      <div className="topbar__right">
        <JobTray />
        <ProviderBadge provider={active} health={health.data} />
        <EngineBadge status={system.data?.engine} />
        <UpdateBadge state={system.data?.update} onClick={() => navigate("/settings#updates")} />
        <Sections label="Everything else" className="topbar__nav" />
      </div>
    </header>
  );
}
