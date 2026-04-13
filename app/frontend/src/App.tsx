import React, { useEffect, useState } from "react";
import { Link, Navigate, Route, Routes, useLocation } from "react-router-dom";
import Dashboard from "./pages/Dashboard";
import Timeline from "./pages/Timeline";
import AutomationDetail from "./pages/AutomationDetail";
import Settings from "./pages/Settings";

type Theme = "light" | "dark";

const NAV_ITEMS = [
  { to: "/", label: "Dashboard", icon: "◆" },
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

  useEffect(() => {
    document.documentElement.setAttribute("data-theme", theme);
    window.localStorage.setItem("flowpilot.theme", theme);
  }, [theme]);

  const toggleTheme = () => setTheme((t) => (t === "dark" ? "light" : "dark"));

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="sidebar-brand">
          <h1>FlowPilot</h1>
          <span>Workflow Intelligence</span>
        </div>

        <nav className="sidebar-nav">
          {NAV_ITEMS.map((item) => (
            <Link
              key={item.to}
              to={item.to}
              className={isActive(location.pathname, item.to) ? "active" : ""}
            >
              <span style={{ fontSize: 16, opacity: 0.7 }}>{item.icon}</span>
              {item.label}
            </Link>
          ))}
        </nav>

        <div className="sidebar-footer">
          <button
            className="theme-toggle"
            onClick={toggleTheme}
            style={{ width: "100%", justifyContent: "center" }}
          >
            {theme === "dark" ? "☀ Light" : "◑ Dark"}
          </button>
        </div>
      </aside>

      <div className="content-area">
        <header className="topbar">
          <span className="discover-badge" style={{ marginRight: "auto", opacity: 0.6 }}>
            <span className="discover-dot" /> Connected
          </span>
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
