import React, { startTransition, useEffect, useRef, useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import {
  createAutomation,
  explainTask,
  getAutomations,
  getTasks,
  type AutomationPlanOut,
  type ExplainTaskOut,
  type TaskOut,
} from "../api";

const TASKS_STORAGE_KEY = "flowpilot.dashboard.tasks.v1";

function readCachedTasks(): TaskOut[] {
  try {
    const raw = sessionStorage.getItem(TASKS_STORAGE_KEY);
    if (!raw) return [];
    const p = JSON.parse(raw) as { at: number; tasks: TaskOut[] };
    if (!p.tasks || !Array.isArray(p.tasks)) return [];
    if (Date.now() - p.at > 15 * 60_000) return [];
    return p.tasks;
  } catch {
    return [];
  }
}

function cacheTasks(tasks: TaskOut[]) {
  try {
    sessionStorage.setItem(TASKS_STORAGE_KEY, JSON.stringify({ at: Date.now(), tasks }));
  } catch {}
}

function friendlyStep(token: string) {
  if (token.startsWith("VIEW:")) return `Open ${token.split(":", 2)[1]}`;
  if (token.startsWith("SUBMIT_TEXT:")) return `Submit "${token.split(":", 2)[1]}"`;
  if (token.startsWith("TYPE_TEXT:")) return `Type "${token.split(":", 2)[1]}"`;
  if (token === "MOVE") return "Move";
  if (token === "screenshot") return "Screenshot";
  if (token.startsWith("CLICK:")) return `Click ${token.split(":", 2)[1]}`;
  if (token.startsWith("KEY:")) return `Press ${token.replace("KEY:", "").replace("<", "").replace(">", "")}`;
  if (token.startsWith("ACTION:")) return token.split(":", 2)[1].split("@", 1)[0].replaceAll("_", " ");
  if (token.startsWith("TYPE_CHAR:")) return `Type '${token.split(":", 2)[1]}'`;
  if (token.startsWith("TYPE_CHAR")) return "Type";
  return token;
}

function taskSummary(task: TaskOut) {
  const filtered = task.steps.filter((x) => x && x !== "MOVE" && x !== "screenshot").slice(0, 8);
  const shown = filtered.length === 0 ? task.steps.slice(0, 6).map(friendlyStep) : filtered.map(friendlyStep);
  const suffix = task.steps.length > shown.length ? " ..." : "";
  return shown.join(" → ") + suffix;
}

function riskBadge(risk: string) {
  const r = risk.toLowerCase();
  if (r === "low") return "badge badge-success";
  if (r === "high") return "badge badge-danger";
  return "badge badge-warning";
}

function confidenceColor(score: number) {
  if (score >= 0.8) return "var(--success)";
  if (score >= 0.5) return "var(--warning)";
  return "var(--text-tertiary)";
}

export default function Dashboard() {
  const [tasks, setTasks] = useState<TaskOut[]>(() => readCachedTasks());
  const [automations, setAutomations] = useState<AutomationPlanOut[]>([]);
  const [taskExplanations, setTaskExplanations] = useState<Record<number, ExplainTaskOut>>({});
  const [explainLoading, setExplainLoading] = useState<Record<number, boolean>>({});
  const [explainErrors, setExplainErrors] = useState<Record<number, string>>({});
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const [discovering, setDiscovering] = useState(false);
  const [minFrequency, setMinFrequency] = useState(2);
  const [lastUpdatedAt, setLastUpdatedAt] = useState("");
  const navigate = useNavigate();
  const discoverLockRef = useRef(false);

  function applyTaskAutos(t: TaskOut[], a: AutomationPlanOut[]) {
    startTransition(() => {
      setTasks(t);
      setAutomations(a);
      cacheTasks(t);
      setLastUpdatedAt(new Date().toLocaleTimeString());
    });
  }

  async function refreshSnapshot(showSpinner: boolean) {
    if (showSpinner) setLoading(true);
    setError("");
    try {
      const [t, a] = await Promise.all([getTasks({ limitLogs: 400, discover: false }), getAutomations()]);
      applyTaskAutos(t, a);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      if (showSpinner) setLoading(false);
    }
  }

  async function refreshWithDiscovery(showSpinner: boolean) {
    if (showSpinner) setLoading(true);
    setError("");
    try {
      const [t, a] = await Promise.all([getTasks({ limitLogs: 400, discover: true }), getAutomations()]);
      applyTaskAutos(t, a);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      if (showSpinner) setLoading(false);
    }
  }

  async function runBackgroundDiscovery() {
    if (discoverLockRef.current) return;
    discoverLockRef.current = true;
    setDiscovering(true);
    setError("");
    try {
      const t = await getTasks({ limitLogs: 400, discover: true });
      applyTaskAutos(t, await getAutomations());
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setDiscovering(false);
      discoverLockRef.current = false;
    }
  }

  useEffect(() => {
    let cancelled = false;
    (async () => {
      await refreshSnapshot(true);
      if (cancelled) return;
      void runBackgroundDiscovery();
    })();
    const poll = window.setInterval(() => void refreshSnapshot(false), 25000);
    const rediscover = window.setInterval(() => {
      if (document.visibilityState === "visible") void runBackgroundDiscovery();
    }, 120000);
    return () => {
      cancelled = true;
      window.clearInterval(poll);
      window.clearInterval(rediscover);
    };
  }, []);

  async function onGenerateAutomation(task: TaskOut) {
    try {
      const existing = await getAutomations({ taskId: task.task_id, limit: 1 });
      if (existing.length > 0) {
        navigate(`/automation/${existing[0].automation_id}`);
        return;
      }
      const plan = await createAutomation(task.task_id);
      navigate(`/automation/${plan.automation_id}`);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  async function onExplainTask(task: TaskOut) {
    setExplainLoading((prev) => ({ ...prev, [task.task_id]: true }));
    setExplainErrors((prev) => ({ ...prev, [task.task_id]: "" }));
    try {
      const explanation = await explainTask({
        task_id: task.task_id,
        task_name: task.name,
        signature: task.signature,
        actions: task.steps,
        repeat_count: task.frequency,
        last_used: task.last_used,
        confidence_score: task.confidence_score,
      });
      setTaskExplanations((prev) => ({ ...prev, [task.task_id]: explanation }));
    } catch (e) {
      setExplainErrors((prev) => ({ ...prev, [task.task_id]: e instanceof Error ? e.message : String(e) }));
    } finally {
      setExplainLoading((prev) => ({ ...prev, [task.task_id]: false }));
    }
  }

  const repeatedTasks = tasks.filter((t) => t.frequency >= minFrequency);

  return (
    <div>
      <div className="page-header">
        <h1>Dashboard</h1>
        <p>Your workflow patterns, detected tasks, and automation suggestions at a glance.</p>
      </div>

      {error && <div className="error-banner">{error}</div>}

      {/* Stats Row */}
      <div className="stats-row">
        <div className="stat-card">
          <span className="stat-value">{tasks.length}</span>
          <span className="stat-label">Detected Tasks</span>
        </div>
        <div className="stat-card">
          <span className="stat-value">{repeatedTasks.length}</span>
          <span className="stat-label">Repeated</span>
        </div>
        <div className="stat-card">
          <span className="stat-value">{automations.length}</span>
          <span className="stat-label">Automations</span>
        </div>
        <div className="stat-card">
          <span className="stat-value" style={{ fontSize: 16, paddingTop: 6 }}>
            {lastUpdatedAt || "--:--"}
          </span>
          <span className="stat-label">Last Updated</span>
        </div>
      </div>

      {/* Toolbar */}
      <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 20 }}>
        <button
          className="btn-primary"
          onClick={() => void refreshWithDiscovery(true)}
          disabled={loading || discovering}
        >
          {loading ? "Refreshing..." : "Refresh"}
        </button>

        {discovering && (
          <span className="discover-badge">
            <div className="discover-dot"></div> Scanning patterns...
          </span>
        )}

        <div style={{ marginLeft: "auto", display: "flex", alignItems: "center", gap: 8 }}>
          <span style={{ fontSize: 12, color: "var(--text-tertiary)" }}>Min frequency</span>
          <select
            value={minFrequency}
            onChange={(e) => setMinFrequency(Number(e.target.value))}
            style={{ padding: "5px 8px", fontSize: 12 }}
          >
            <option value={1}>1+</option>
            <option value={2}>2+</option>
            <option value={5}>5+</option>
            <option value={10}>10+</option>
          </select>
        </div>
      </div>

      {/* Repeated Tasks */}
      <div style={{ marginBottom: 32 }}>
        <h3 className="section-title">Repeated Tasks</h3>

        {loading && tasks.length === 0 ? (
          <div style={{ display: "grid", gap: 12 }}>
            {[1, 2, 3].map((i) => (
              <div key={i} className="skeleton" style={{ height: 100, animationDelay: `${i * 120}ms` }} />
            ))}
          </div>
        ) : repeatedTasks.length === 0 ? (
          <div className="empty-state">
            <h3>No repeated tasks found</h3>
            <p>Perform some actions and check back in a moment. FlowPilot will detect patterns automatically.</p>
          </div>
        ) : (
          <div className="task-list">
            {repeatedTasks.slice(0, 12).map((t, idx) => (
              <div
                key={t.task_id}
                className="task-card"
                style={{ animationDelay: `${Math.min(idx, 10) * 50}ms` }}
              >
                <div className="task-card-header">
                  <span className="task-card-name">{t.name?.trim() || `Workflow ${t.task_id}`}</span>
                  <div className="task-card-meta">
                    <span className="badge badge-accent">{t.frequency}×</span>
                    <span
                      className="badge"
                      style={{
                        background: "transparent",
                        color: confidenceColor(t.confidence_score),
                        border: `1px solid ${confidenceColor(t.confidence_score)}`,
                      }}
                    >
                      {(t.confidence_score * 100).toFixed(0)}%
                    </span>
                  </div>
                </div>

                <div className="task-card-summary">{taskSummary(t)}</div>

                {/* Explain Panel */}
                {taskExplanations[t.task_id] && (
                  <div className="explain-panel">
                    {taskExplanations[t.task_id].explanation}
                    {taskExplanations[t.task_id].used_fallback && (
                      <div style={{ marginTop: 10, fontSize: 11, color: "var(--text-tertiary)", fontStyle: "italic" }}>
                        Local analysis — LLM unavailable
                      </div>
                    )}
                  </div>
                )}

                {explainErrors[t.task_id] && (
                  <div style={{ marginTop: 10, fontSize: 13, color: "var(--danger)" }}>
                    {explainErrors[t.task_id]}
                  </div>
                )}

                <div className="task-card-actions">
                  <button className="btn-sm" onClick={() => void onExplainTask(t)} disabled={!!explainLoading[t.task_id]}>
                    {explainLoading[t.task_id] ? "Analyzing..." : "✦ Explain"}
                  </button>
                  <button className="btn-sm" onClick={() => onGenerateAutomation(t)}>
                    ⚡ Automate
                  </button>
                  <Link to="/timeline" style={{ fontSize: 12, padding: "5px 10px" }}>
                    View activity →
                  </Link>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>

      {/* Automations */}
      <div>
        <h3 className="section-title">Automation Suggestions</h3>
        {automations.length === 0 ? (
          <div className="empty-state">
            <h3>No automations yet</h3>
            <p>Click "Automate" on a repeated task above to generate your first workflow automation.</p>
          </div>
        ) : (
          <div className="task-list">
            {automations.slice(0, 10).map((a, idx) => (
              <Link
                key={a.automation_id}
                to={`/automation/${a.automation_id}`}
                className="task-card"
                style={{
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "space-between",
                  textDecoration: "none",
                  color: "var(--text)",
                  animationDelay: `${idx * 50}ms`,
                }}
              >
                <span style={{ fontWeight: 500, fontSize: 14 }}>{a.name}</span>
                <span className={riskBadge(a.risk_level)}>{a.risk_level}</span>
              </Link>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
