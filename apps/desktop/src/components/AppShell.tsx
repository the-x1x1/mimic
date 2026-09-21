import { useEffect } from "react";
import { Outlet, useLocation, useNavigate } from "react-router-dom";
import { useSystemStatus } from "@/hooks/useSystem";
import { useProviderHealth, useProviderState } from "@/hooks/useCompose";
import { EngineBadge, ProviderBadge, UpdateBadge } from "./StatusBadges";
import { JobTray } from "./JobTray";
import { Toaster } from "./Toaster";
import { RepliesPage } from "@/features/replies/RepliesPage";

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
 */
export function AppShell() {
  const location = useLocation();
  const navigate = useNavigate();
  const open = location.pathname !== "/";

  // Escape closes the drawer, because a panel that can only be dismissed with
  // a small × is a panel someone gets stuck in.
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") navigate("/");
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, navigate]);

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
          <aside className="drawer" role="dialog" aria-modal="true" aria-label="Settings">
            <button
              type="button"
              className="drawer__close"
              aria-label="Close and go back to your replies"
              onClick={() => navigate("/")}
            >
              Back to replies
            </button>
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
        <button type="button" className="topbar__settings" onClick={() => navigate("/settings")}>
          Settings
        </button>
      </div>
    </header>
  );
}
