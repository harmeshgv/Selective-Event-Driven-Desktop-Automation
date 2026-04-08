import React from "react";
import { Link, Navigate, Route, Routes } from "react-router-dom";
import Dashboard from "./pages/Dashboard";
import Timeline from "./pages/Timeline";
import AutomationDetail from "./pages/AutomationDetail";
import Settings from "./pages/Settings";

export default function App() {
  return (
    <div style={{ fontFamily: "system-ui, -apple-system, Segoe UI, Roboto, Arial" }}>
      <header
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          padding: 12,
          borderBottom: "1px solid #eee",
        }}
      >
        <div style={{ fontWeight: 700 }}>FlowPilot</div>
        <nav style={{ display: "flex", gap: 12 }}>
          <Link to="/">Dashboard</Link>
          <Link to="/timeline">Activity Timeline</Link>
          <Link to="/settings">Tracking & Privacy</Link>
        </nav>
      </header>

      <Routes>
        <Route path="/" element={<Dashboard />} />
        <Route path="/timeline" element={<Timeline />} />
        <Route path="/automation/:automationId" element={<AutomationDetail />} />
        <Route path="/settings" element={<Settings />} />
        <Route path="*" element={<Navigate to="/" replace />} />
      </Routes>
    </div>
  );
}

