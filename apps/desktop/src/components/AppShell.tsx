import { NavLink, Outlet, useNavigate } from "react-router-dom";
import { PenLine, Users, AudioLines, Inbox, Settings as SettingsIcon } from "lucide-react";
import { useNativeEventBridge, useSystemStatus } from "@/hooks/useSystem";
import { useProviderState } from "@/hooks/useCompose";
import { EngineBadge, ProviderBadge, UpdateBadge } from "./StatusBadges";
import { JobTray } from "./JobTray";
import { Toaster } from "./Toaster";

const NAV = [
  { to: "/", label: "Compose", icon: PenLine, end: true },
  { to: "/people", label: "People", icon: Users },
  { to: "/voice", label: "Voice", icon: AudioLines },
  { to: "/sources", label: "Sources", icon: Inbox },
  { to: "/settings", label: "Settings", icon: SettingsIcon },
];

export function AppShell() {
  useNativeEventBridge();
  const system = useSystemStatus();
  const providers = useProviderState();
  const navigate = useNavigate();
  const active = providers.data?.providers.find((p) => p.id === providers.data?.active);
  return (
    <div className="shell">
      <aside className="sidebar" aria-label="Primary">
        <div className="brand">
          <span className="brand__mark" aria-hidden />
          <span className="brand__name">Mimic</span>
        </div>
        <nav className="nav">
          {NAV.map(({ to, label, icon: Icon, end }) => (
            <NavLink
              key={to}
              to={to}
              end={end}
              className={({ isActive }) => (isActive ? "nav__item nav__item--active" : "nav__item")}
            >
              <Icon size={18} aria-hidden />
              <span>{label}</span>
            </NavLink>
          ))}
        </nav>
        <div className="sidebar__footer">
          <JobTray />
        </div>
      </aside>
      <div className="main">
        <header className="topbar">
          <div className="topbar__status">
            <ProviderBadge provider={active} />
            <EngineBadge status={system.data?.engine} />
          </div>
          <div className="topbar__right">
            <UpdateBadge state={system.data?.update} onClick={() => navigate("/settings")} />
          </div>
        </header>
        <main className="content">
          <Outlet />
        </main>
      </div>
      <Toaster />
    </div>
  );
}
