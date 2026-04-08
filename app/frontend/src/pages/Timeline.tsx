import React, { useEffect, useMemo, useState } from "react";
import type { LogOut } from "../api";
import { getLogs } from "../api";

function formatTime(iso: string) {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return d.toLocaleString();
}

export default function Timeline() {
  const [logs, setLogs] = useState<LogOut[]>([]);
  const [error, setError] = useState<string>("");
  const [loading, setLoading] = useState<boolean>(false);
  const [limit, setLimit] = useState<number>(200);

  const fetchLogs = async (l: number) => {
    setLoading(true);
    setError("");
    try {
      const res = await getLogs(l);
      setLogs(res);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchLogs(limit);
    const t = window.setInterval(() => fetchLogs(limit), 5000);
    return () => window.clearInterval(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [limit]);

  const rows = useMemo(() => logs.slice().sort((a, b) => a.id - b.id), [logs]);

  return (
    <div style={{ padding: 16 }}>
      <h1 style={{ marginTop: 0 }}>Activity Timeline</h1>

      <div style={{ display: "flex", gap: 12, alignItems: "center", marginBottom: 12 }}>
        <label>
          Show last{" "}
          <select value={limit} onChange={(e) => setLimit(Number(e.target.value))}>
            <option value={100}>100</option>
            <option value={200}>200</option>
            <option value={500}>500</option>
          </select>{" "}
          events
        </label>

        <button onClick={() => fetchLogs(limit)} disabled={loading}>
          {loading ? "Refreshing..." : "Refresh"}
        </button>
      </div>

      {error ? (
        <pre style={{ color: "crimson", whiteSpace: "pre-wrap" }}>{error}</pre>
      ) : null}

      <section>
        <pre style={{ background: "#f6f6f6", padding: 12, overflow: "auto", maxHeight: 640 }}>
          {rows.length === 0
            ? "No logs yet."
            : rows
                .map((l) => {
                  const line = `[${formatTime(l.timestamp)}] ${l.app} | ${l.action} | ${l.coordinates}`;
                  const text = l.text ? ` | text=${l.text}` : "";
                  return line + text;
                })
                .join("\n")}
        </pre>
      </section>
    </div>
  );
}

