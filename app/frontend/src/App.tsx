import React, { useEffect, useState } from "react";
import { Link, Navigate, Route, Routes, useLocation } from "react-router-dom";
import Dashboard from "./pages/Dashboard";
import Timeline from "./pages/Timeline";
import AutomationDetail from "./pages/AutomationDetail";
import Settings from "./pages/Settings";

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

  useEffect(() => {
    document.documentElement.setAttribute("data-theme", theme);
    window.localStorage.setItem("flowpilot.theme", theme);
  }, [theme]);

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
            <div className="discover-dot"></div>
            Observer Active
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
