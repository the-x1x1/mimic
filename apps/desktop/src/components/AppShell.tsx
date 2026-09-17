import { NavLink, Outlet, useNavigate } from "react-router-dom";
import { Home, Layers, Images, ClipboardCheck, Settings as SettingsIcon } from "lucide-react";
import { useNativeEventBridge, useSystemStatus } from "@/hooks/useSystem";
import { ConnectionBadge, EngineBadge, UpdateBadge } from "./StatusBadges";
import { JobTray } from "./JobTray";
import { Toaster } from "./Toaster";

const NAV = [
  { to: "/", label: "Home", icon: Home, end: true },
  { to: "/styles", label: "Styles", icon: Layers },
  { to: "/sessions", label: "Sessions", icon: Images },
  { to: "/review", label: "Review", icon: ClipboardCheck },
  { to: "/settings", label: "Settings", icon: SettingsIcon },
];

export function AppShell() {
  useNativeEventBridge();
  const system = useSystemStatus();
  const navigate = useNavigate();
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
            <ConnectionBadge status={system.data?.lightroom} />
            <EngineBadge status={system.data?.engine} />
          </div>
          <div className="topbar__right">
            <UpdateBadge
              state={system.data?.update}
              onClick={() => navigate("/settings?section=updates")}
            />
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
