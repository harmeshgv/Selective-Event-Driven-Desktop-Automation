import React, { useEffect, useState } from "react";
import { Link, Navigate, Route, Routes, useLocation } from "react-router-dom";
import Dashboard from "./pages/Dashboard";
import Timeline from "./pages/Timeline";
import AutomationDetail from "./pages/AutomationDetail";
import Settings from "./pages/Settings";
import { getObserverSettings, type ObserverSettingsOut } from "./api";

type Theme = "light" | "dark";

const NAV_ITEMS = [
  { to: "/", label: "Dashboard", icon: "◫" },
  { to: "/timeline", label: "Timeline", icon: "◷" },
  { to: "/settings", label: "Settings", icon: "⚙" },
];

function isActive(path: string, to: string) {
  if (to === "/") return path === "/";
  return path.startsWith(to);
}

export default function App() {
  const location = useLocation();
  const [theme, setTheme] = useState<Theme>(() => {
    const stored = window.localStorage.getItem("flowpilot.theme");
    if (stored === "light" || stored === "dark") return stored;
    return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  });
  const [observerStatus, setObserverStatus] = useState<ObserverSettingsOut | null>(null);

  useEffect(() => {
    document.documentElement.setAttribute("data-theme", theme);
    window.localStorage.setItem("flowpilot.theme", theme);
  }, [theme]);

  useEffect(() => {
    let cancelled = false;
    const poll = () => {
      getObserverSettings()
        .then((s) => { if (!cancelled) setObserverStatus(s); })
        .catch(() => {});
    };
    poll();
    const id = window.setInterval(poll, 5000);
    return () => { cancelled = true; window.clearInterval(id); };
  }, []);

  const toggleTheme = () => setTheme((t) => (t === "dark" ? "light" : "dark"));

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="sidebar-brand">
          <div className="pilot-icon"></div>
          <h1>FlowPilot</h1>
        </div>

        <nav className="sidebar-nav">
          {NAV_ITEMS.map((item) => (
            <Link
              key={item.to}
              to={item.to}
              className={isActive(location.pathname, item.to) ? "active" : ""}
            >
              <span style={{ fontSize: 16, marginRight: 8, display: "flex", alignItems: "center" }}>{item.icon}</span>
              {item.label}
            </Link>
          ))}
        </nav>

        <div className="sidebar-footer" style={{ marginTop: "auto", paddingTop: 16 }}>
          <button
            onClick={toggleTheme}
            style={{ width: "100%", justifyContent: "center", background: "transparent", border: "1px solid var(--border)", boxShadow: "none" }}
          >
            {theme === "dark" ? "Light Mode" : "Dark Mode"}
          </button>
        </div>
      </aside>

      <div className="content-area">
        <header className="topbar">
          <span className="discover-badge" style={{ marginRight: "auto" }}>
            <div
              className="discover-dot"
              style={{
                background: observerStatus?.tracking_enabled ? "var(--accent)" : "var(--text-tertiary)",
                boxShadow: observerStatus?.tracking_enabled ? "0 0 4px var(--accent)" : "none",
              }}
            />
            {observerStatus == null
              ? "Connecting..."
              : observerStatus.tracking_enabled
                ? observerStatus.privacy_mode
                  ? "Privacy Mode"
                  : "Recording"
                : "Paused"}
          </span>
          <div style={{ display: "flex", gap: "12px", alignItems: "center" }}>
            <div style={{ width: 28, height: 28, borderRadius: "50%", background: "var(--bg-inset)", color: "var(--text-secondary)", display: "flex", alignItems: "center", justifyContent: "center", fontWeight: "bold", fontSize: 11, border: "1px solid var(--border)" }}>U</div>
          </div>
        </header>

        <main className="content-shell">
          <Routes>
            <Route path="/" element={<Dashboard />} />
            <Route path="/timeline" element={<Timeline />} />
            <Route path="/automation/:automationId" element={<AutomationDetail />} />
            <Route path="/settings" element={<Settings />} />
            <Route path="*" element={<Navigate to="/" replace />} />
          </Routes>
        </main>
      </div>
    </div>
  );
}
