import React, { useEffect, useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import {
  createAutomation,
  getAutomations,
  getTasks,
  type AutomationPlanOut,
  type TaskOut,
} from "../api";

export default function Dashboard() {
  const [tasks, setTasks] = useState<TaskOut[]>([]);
  const [automations, setAutomations] = useState<AutomationPlanOut[]>([]);
  const [error, setError] = useState<string>("");
  const [loading, setLoading] = useState<boolean>(false);
  const [minFrequency, setMinFrequency] = useState<number>(2);
  const navigate = useNavigate();

  async function refresh() {
    setLoading(true);
    setError("");
    try {
      const [t, a] = await Promise.all([getTasks(), getAutomations()]);
      setTasks(t);
      setAutomations(a);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    refresh();
    const t = window.setInterval(() => refresh(), 15000);
    return () => window.clearInterval(t);
  }, []);

  function friendlyStep(token: string) {
    if (token.startsWith("VIEW:")) return `Open ${token.split(":", 2)[1]}`;
    if (token.startsWith("SUBMIT_TEXT:")) return `Submit "${token.split(":", 2)[1]}"`;
    if (token.startsWith("TYPE_TEXT:")) return `Type "${token.split(":", 2)[1]}"`;
    if (token === "MOVE") return "Move mouse";
    if (token === "screenshot") return "Take screenshot";
    if (token.startsWith("CLICK:")) return `Click ${token.split(":", 2)[1]}`;
    if (token.startsWith("KEY:")) {
      const k = token.replace("KEY:", "");
      return `Press ${k.replace("<", "").replace(">", "")}`;
    }
    if (token.startsWith("ACTION:")) return token.split(":", 2)[1].split("@", 1)[0].replaceAll("_", " ");
    if (token.startsWith("TYPE_CHAR:")) return `Type '${token.split(":", 2)[1]}'`;
    if (token.startsWith("TYPE_TEXT:")) return `Type "${token.split(":", 2)[1]}"`;
    if (token.startsWith("TYPE_CHAR")) return "Type";
    return token;
  }

  function taskSummary(task: TaskOut) {
    const displayTokens = task.steps
      .filter((x) => x && x !== "MOVE" && x !== "screenshot")
      .slice(0, 8);
    const shown =
      displayTokens.length === 0
        ? task.steps.slice(0, 6).map(friendlyStep)
        : displayTokens.map(friendlyStep);
    const suffix = task.steps.length > shown.length ? " …" : "";
    return shown.join(" • ") + suffix;
  }

  function labelForRisk(risk: string) {
    const x = risk.toLowerCase();
    if (x === "low") return { text: "Low risk", color: "#1b9e3e" };
    if (x === "high") return { text: "High risk", color: "#d62828" };
    return { text: "Medium risk", color: "#f59f00" };
  }

  async function onGenerateAutomation(task: TaskOut) {
    try {
      const plan = await createAutomation(task.task_id);
      navigate(`/automation/${plan.automation_id}`);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  const repeatedTasks = tasks.filter((t) => t.frequency >= minFrequency);

  return (
    <div style={{ padding: 16 }}>
      <h1 style={{ margin: 0 }}>FlowPilot Dashboard</h1>
      {error ? (
        <pre style={{ color: "crimson", whiteSpace: "pre-wrap" }}>{error}</pre>
      ) : null}

      <div style={{ marginTop: 12, marginBottom: 12, display: "flex", gap: 12, alignItems: "center" }}>
        <div>
          <strong>{automations.length}</strong> automations
          <span style={{ color: "#666" }}> · </span>
          <strong>{tasks.length}</strong> tasks
        </div>
        <button onClick={() => refresh()} disabled={loading}>
          {loading ? "Refreshing..." : "Refresh"}
        </button>
        <Link to="/timeline">Go to timeline</Link>
      </div>

      <section style={{ marginTop: 24 }}>
        <h2>Repeated Tasks</h2>
        <div style={{ display: "flex", gap: 12, alignItems: "center", marginBottom: 12 }}>
          <label>
            Show tasks with frequency at least{" "}
            <select value={minFrequency} onChange={(e) => setMinFrequency(Number(e.target.value))}>
              <option value={1}>1+</option>
              <option value={2}>2+</option>
              <option value={5}>5+</option>
              <option value={10}>10+</option>
            </select>
          </label>
        </div>

        {repeatedTasks.length === 0 ? (
          <div>No repeated workflows found yet. Do some common actions and come back in ~30 seconds.</div>
        ) : (
          <div style={{ display: "grid", gap: 12 }}>
            {repeatedTasks.slice(0, 10).map((t) => (
              <div
                key={t.task_id}
                style={{
                  border: "1px solid #eee",
                  borderRadius: 10,
                  padding: 12,
                  background: "#fff",
                }}
              >
                <div style={{ display: "flex", justifyContent: "space-between", gap: 12, alignItems: "baseline" }}>
                  <div style={{ fontWeight: 700 }}>
                    {t.name && t.name.trim() ? t.name : `Workflow ${t.task_id}`}
                  </div>
                  <div style={{ fontSize: 13, color: "#666", textAlign: "right" }}>
                    seen <strong style={{ color: "#111" }}>{t.frequency}×</strong>
                    <div>confidence {(t.confidence_score * 100).toFixed(0)}%</div>
                  </div>
                </div>

                <div style={{ marginTop: 6, color: "#444" }}>{taskSummary(t)}</div>

                <div style={{ marginTop: 10, display: "flex", gap: 10, flexWrap: "wrap" }}>
                  <button onClick={() => onGenerateAutomation(t)}>
                    Suggest automation
                  </button>
                  <Link to="/timeline">View activity</Link>
                </div>
              </div>
            ))}
          </div>
        )}
      </section>

      <section style={{ marginTop: 24 }}>
        <h2>Automation Suggestions</h2>
        {automations.length === 0 ? (
          <div>No automations found yet. Start by generating logs, or use the seeded demo.</div>
        ) : (
          <ul>
            {automations.slice(0, 10).map((a) => (
              <li key={a.automation_id}>
                <Link to={`/automation/${a.automation_id}`}>
                  {a.name} (risk: {a.risk_level})
                </Link>
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}

