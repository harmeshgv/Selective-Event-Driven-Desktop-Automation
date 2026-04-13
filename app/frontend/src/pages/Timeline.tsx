import React, { startTransition, useEffect, useMemo, useRef, useState } from "react";
import type { LogOut } from "../api";
import { getLogs } from "../api";

const TIMELINE_CACHE_KEY = "flowpilot.timeline.logs.v1";

function formatTime(iso: string) {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return d.toLocaleString(undefined, { hour: "2-digit", minute: "2-digit", second: "2-digit" });
}

function formatDate(iso: string) {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  return d.toLocaleDateString(undefined, { weekday: "short", month: "short", day: "numeric" });
}

function actionColor(action: string): string {
  const a = action.toLowerCase();
  if (a.includes("click")) return "var(--accent)";
  if (a.includes("key") || a.includes("type")) return "var(--warning)";
  if (a.includes("view") || a.includes("screenshot")) return "var(--success)";
  if (a.includes("move")) return "var(--text-tertiary)";
  return "var(--accent)";
}

function sortLogsAscending(rows: LogOut[]) {
  return rows.slice().sort((a, b) => a.id - b.id);
}

function mergeRecentLogs(existing: LogOut[], incoming: LogOut[], limit: number) {
  if (incoming.length === 0) return existing;
  const merged = new Map<number, LogOut>();
  for (const row of existing) merged.set(row.id, row);
  for (const row of incoming) merged.set(row.id, row);
  const ordered = Array.from(merged.values()).sort((a, b) => a.id - b.id);
  return ordered.length > limit ? ordered.slice(-limit) : ordered;
}

function readCachedLogs(limit: number): LogOut[] {
  try {
    const raw = sessionStorage.getItem(TIMELINE_CACHE_KEY);
    if (!raw) return [];
    const p = JSON.parse(raw) as { at: number; limit: number; logs: LogOut[] };
    if (!p.logs || !Array.isArray(p.logs)) return [];
    if (p.limit !== limit) return [];
    if (Date.now() - p.at > 20 * 60_000) return [];
    return p.logs;
  } catch {
    return [];
  }
}

function cacheLogs(logs: LogOut[], limit: number) {
  try {
    sessionStorage.setItem(TIMELINE_CACHE_KEY, JSON.stringify({ at: Date.now(), limit, logs }));
  } catch {}
}

const TimelineRow = React.memo(function TimelineRow({ log, idx }: { log: LogOut; idx: number }) {
  const color = actionColor(log.action);
  return (
    <div className="timeline-event" style={{ animationDelay: `${Math.min(idx, 40) * 25}ms` }}>
      <div className="timeline-time">{formatTime(log.timestamp)}</div>
      <div className="timeline-body">
        <span className="timeline-action">{log.action}</span>
        <span>{log.app}</span>
        {log.text && (
          <>
            <span style={{ color: "var(--text-tertiary)" }}>{log.text}</span>
          </>
        )}
      </div>
    </div>
  );
});

export default function Timeline() {
  const [limit, setLimitState] = useState<number>(() => {
    try {
      const s = sessionStorage.getItem("flowpilot.timeline.limit.v1");
      return s ? Number(s) : 200;
    } catch {
      return 200;
    }
  });

  const [logs, setLogs] = useState<LogOut[]>(() => readCachedLogs(limit));
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const latestIdRef = useRef<number | null>(null);

  const orderedForDisplay = useMemo(() => sortLogsAscending(logs), [logs]);

  const fetchLogs = async ({ mode, showSpinner }: { mode: "replace" | "append"; showSpinner: boolean }) => {
    if (showSpinner) setLoading(true);
    setError("");
    try {
      const sinceId = mode === "append" ? latestIdRef.current ?? undefined : undefined;
      const res = await getLogs({ limit, sinceId });
      startTransition(() => {
        setLogs((current) => {
          const next =
            mode === "append" ? mergeRecentLogs(current, res, limit) : sortLogsAscending(res).slice(-limit);
          latestIdRef.current = next.length > 0 ? next[next.length - 1].id : null;
          cacheLogs(next, limit);
          return next;
        });
      });
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      if (showSpinner) setLoading(false);
    }
  };

  function setLimit(next: number) {
    setLimitState(next);
    try {
      sessionStorage.setItem("flowpilot.timeline.limit.v1", String(next));
    } catch {}
  }

  useEffect(() => {
    latestIdRef.current = null;
    const cached = readCachedLogs(limit);
    setLogs(cached);
    if (cached.length > 0) {
      const sorted = sortLogsAscending(cached);
      latestIdRef.current = sorted[sorted.length - 1].id;
    }
    void fetchLogs({ mode: "replace", showSpinner: cached.length === 0 });
    const t = window.setInterval(() => void fetchLogs({ mode: "append", showSpinner: false }), 12000);
    return () => window.clearInterval(t);
  }, [limit]);

  return (
    <div>
      <div className="page-header">
        <h1>Activity Timeline</h1>
        <p>Live event stream from your desktop. New events flow in automatically every 12 seconds.</p>
      </div>

      {error && <div className="error-banner">{error}</div>}

      {/* Toolbar */}
      <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 20 }}>
        <button className="btn-primary" onClick={() => void fetchLogs({ mode: "replace", showSpinner: true })} disabled={loading}>
          {loading ? "Loading..." : "Refresh"}
        </button>

        <div style={{ marginLeft: "auto", display: "flex", alignItems: "center", gap: 8 }}>
          <span style={{ fontSize: 12, color: "var(--text-tertiary)" }}>Show last</span>
          <select
            value={limit}
            onChange={(e) => setLimit(Number(e.target.value))}
            style={{ padding: "5px 8px", fontSize: 12 }}
          >
            <option value={100}>100</option>
            <option value={200}>200</option>
            <option value={500}>500</option>
          </select>
          <span style={{ fontSize: 12, color: "var(--text-tertiary)" }}>events</span>
        </div>

        <span className="badge badge-muted" style={{ fontSize: 12 }}>
          {orderedForDisplay.length} loaded
        </span>
      </div>

      {/* Event List */}
      {loading && logs.length === 0 ? (
        <div style={{ display: "grid", gap: 8 }}>
          {[1, 2, 3, 4, 5].map((i) => (
            <div key={i} className="skeleton" style={{ height: 44, animationDelay: `${i * 100}ms` }} />
          ))}
        </div>
      ) : orderedForDisplay.length === 0 ? (
        <div className="empty-state">
          <h3>No events yet</h3>
          <p>Start recording in Settings. Events will appear here in real-time.</p>
        </div>
      ) : (
        <div
          className="task-list"
          style={{
            maxHeight: "calc(100vh - 260px)",
            overflowY: "auto",
          }}
        >
          {orderedForDisplay.map((l, i) => (
            <TimelineRow key={l.id} log={l} idx={i} />
          ))}
        </div>
      )}
    </div>
  );
}
