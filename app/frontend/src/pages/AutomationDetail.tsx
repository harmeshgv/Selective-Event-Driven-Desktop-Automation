import React, { useEffect, useRef, useState } from "react";
import { Link, useParams } from "react-router-dom";
import {
  getAutomation,
  type AutomationStepOut,
  runAutomation,
  runSmartAutomation,
  updateAutomationSteps,
  type AutomationPlanOut,
  type RunAutomationOut,
  type SmartRunStepEvent,
  type SmartRunDoneEvent,
} from "../api";

function riskBadgeClass(risk: string) {
  const r = risk.toLowerCase();
  if (r === "low") return "badge badge-success";
  if (r === "high") return "badge badge-danger";
  return "badge badge-warning";
}

function statusIcon(status: string) {
  if (status === "success") return "\u2705";
  if (status === "failed") return "\u274C";
  if (status === "running") return "\u23F3";
  if (status === "corrected") return "\uD83D\uDD04";
  if (status === "pending") return "\u25CB";
  if (status === "would_execute") return "\u25CB";
  return "\u25CB";
}

function statusColor(status: string) {
  if (status === "success") return "var(--success, #22c55e)";
  if (status === "failed") return "var(--danger, #ef4444)";
  if (status === "running") return "var(--warning, #f59e0b)";
  if (status === "corrected") return "var(--warning, #f59e0b)";
  return "var(--text-tertiary, #888)";
}

export default function AutomationDetail() {
  const { automationId } = useParams();
  const id = Number(automationId);

  const [automation, setAutomation] = useState<AutomationPlanOut | null>(null);
  const [editedSteps, setEditedSteps] = useState<AutomationStepOut[]>([]);
  const [preview, setPreview] = useState<RunAutomationOut | null>(null);
  const [approved, setApproved] = useState(false);
  const [error, setError] = useState("");
  const [running, setRunning] = useState(false);

  // Smart execution state
  const [smartMode, setSmartMode] = useState(true);
  const [liveSteps, setLiveSteps] = useState<SmartRunStepEvent[]>([]);
  const [smartDone, setSmartDone] = useState<SmartRunDoneEvent | null>(null);
  const [smartRunning, setSmartRunning] = useState(false);
  const liveLogRef = useRef<HTMLDivElement>(null);

  function buildLocalPreview(steps: AutomationStepOut[]): RunAutomationOut {
    return {
      automation_id: id,
      plan_name: automation?.name ?? "Automation",
      preview: true,
      risk_level: preview?.risk_level ?? automation?.risk_level ?? "unknown",
      status: "previewed",
      error: "",
      steps: steps.map((s) => ({
        step_order: s.step_order,
        description: s.description,
        status: "would_execute",
        attempts: 0,
        error: "",
      })),
    };
  }

  useEffect(() => {
    if (!Number.isFinite(id) || id <= 0) {
      setError("Invalid automation id");
      return;
    }
    (async () => {
      try {
        const found = await getAutomation(id);
        setAutomation(found);
        setEditedSteps(found.steps);
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      }
    })();
  }, [id]);

  useEffect(() => {
    if (!Number.isFinite(id) || id <= 0) return;
    (async () => {
      try {
        const res = await runAutomation({ automation_id: id, preview: true, approved: false });
        setPreview(res);
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      }
    })();
  }, [id]);

  useEffect(() => {
    if (liveLogRef.current) {
      liveLogRef.current.scrollTop = liveLogRef.current.scrollHeight;
    }
  }, [liveSteps]);

  async function refreshPreview() {
    const res = await runAutomation({ automation_id: id, preview: true, approved: false });
    setPreview(res);
  }

  async function onSaveSteps() {
    if (!automation) return;
    setRunning(true);
    setError("");
    try {
      const updated = await updateAutomationSteps(
        id,
        editedSteps.map((s) => ({
          step_id: s.step_id,
          step_order: s.step_order,
          description: s.description,
          action_type: s.action_type,
          target: s.target,
          value: s.value,
          retry_count: s.retry_count,
        })),
      );
      setAutomation(updated);
      setEditedSteps(updated.steps);
      setApproved(false);
      await refreshPreview();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setRunning(false);
    }
  }

  function onDeleteStep(stepId: number) {
    setEditedSteps((prev) => {
      const next = prev.filter((x) => x.step_id !== stepId);
      const renumbered = next.map((x, idx) => ({ ...x, step_order: idx + 1 }));
      setPreview(buildLocalPreview(renumbered));
      return renumbered;
    });
    setApproved(false);
  }

  async function onExecute() {
    if (!automation) return;

    if (smartMode) {
      onSmartExecute();
      return;
    }

    setRunning(true);
    setError("");
    try {
      const res = await runAutomation({ automation_id: id, preview: false, approved });
      setPreview(res);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setRunning(false);
    }
  }

  async function onSmartExecute() {
    if (!automation) return;
    setSmartRunning(true);
    setRunning(true);
    setError("");
    setLiveSteps([]);
    setSmartDone(null);

    await runSmartAutomation(id, {
      onStart: () => {
        setLiveSteps([]);
      },
      onStep: (step) => {
        setLiveSteps((prev) => {
          const existing = prev.findIndex(
            (s) => s.step_order === step.step_order && s.status === step.status && s.attempts === step.attempts,
          );
          if (existing >= 0) {
            const next = [...prev];
            next[existing] = step;
            return next;
          }
          return [...prev, step];
        });
      },
      onDone: (done) => {
        setSmartDone(done);
        setSmartRunning(false);
        setRunning(false);
      },
      onError: (err) => {
        setError(err);
        setSmartRunning(false);
        setRunning(false);
      },
    });

    if (!error) {
      setSmartRunning(false);
      setRunning(false);
    }
  }

  const risk = preview?.risk_level ?? automation?.risk_level ?? "unknown";
  const isRunning = running || smartRunning;

  return (
    <div>
      {/* Breadcrumb */}
      <div style={{ marginBottom: 16 }}>
        <Link to="/" style={{ fontSize: 13, color: "var(--text-tertiary)" }}>
          \u2190 Back to Dashboard
        </Link>
      </div>

      <div className="page-header" style={{ display: "flex", alignItems: "center", gap: 14 }}>
        <div style={{ flex: 1 }}>
          <h1>{automation?.name ?? `Automation #${id}`}</h1>
          <p>Review, edit, and execute this automation workflow.</p>
        </div>
        <span className={riskBadgeClass(risk)} style={{ fontSize: 13, padding: "5px 14px" }}>
          {risk} risk
        </span>
      </div>

      {error && <div className="error-banner">{error}</div>}

      {/* Approval + Execute */}
      <div className="card" style={{ marginBottom: 20, display: "flex", alignItems: "center", gap: 16, flexWrap: "wrap" }}>
        <label style={{ display: "flex", alignItems: "center", gap: 8, cursor: "pointer" }}>
          <button
            className="toggle-switch"
            data-on={String(approved)}
            onClick={() => setApproved((v) => !v)}
            disabled={isRunning}
            type="button"
          />
          <span style={{ fontSize: 14, fontWeight: 500 }}>I approve execution</span>
        </label>

        <label style={{ display: "flex", alignItems: "center", gap: 8, cursor: "pointer" }}>
          <button
            className="toggle-switch"
            data-on={String(smartMode)}
            onClick={() => setSmartMode((v) => !v)}
            disabled={isRunning}
            type="button"
          />
          <span style={{ fontSize: 14, fontWeight: 500 }}>
            AI-powered
          </span>
          <span style={{ fontSize: 11, color: "var(--text-tertiary)" }}>
            (LLM generates & corrects steps)
          </span>
        </label>

        <button
          className="btn-primary"
          onClick={onExecute}
          disabled={!approved || isRunning}
          style={{ marginLeft: "auto" }}
        >
          {isRunning
            ? smartMode
              ? "AI Executing..."
              : "Executing..."
            : smartMode
              ? "AI Execute"
              : "Execute Automation"}
        </button>
      </div>

      {/* Smart Execution Live View */}
      {(liveSteps.length > 0 || smartDone) && (
        <div style={{ marginBottom: 24 }}>
          <h3 className="section-title">
            Live Execution {smartRunning && <span style={{ color: "var(--warning)", fontSize: 13 }}> \u2014 running</span>}
          </h3>

          {smartDone && (
            <div
              className="card"
              style={{
                marginBottom: 12,
                padding: "12px 16px",
                borderLeft: `4px solid ${smartDone.status === "success" ? "var(--success, #22c55e)" : "var(--danger, #ef4444)"}`,
              }}
            >
              <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                <span style={{ fontSize: 20 }}>{smartDone.status === "success" ? "\u2705" : "\u274C"}</span>
                <div>
                  <div style={{ fontWeight: 600, fontSize: 15 }}>
                    {smartDone.status === "success" ? "Automation completed successfully" : "Automation failed"}
                  </div>
                  <div style={{ fontSize: 12, color: "var(--text-tertiary)", marginTop: 2 }}>
                    {smartDone.completed_steps}/{smartDone.total_steps} steps completed
                  </div>
                  {smartDone.error && (
                    <div style={{ fontSize: 12, color: "var(--danger)", marginTop: 4 }}>{smartDone.error}</div>
                  )}
                </div>
              </div>
            </div>
          )}

          <div
            ref={liveLogRef}
            style={{
              maxHeight: 420,
              overflow: "auto",
              display: "flex",
              flexDirection: "column",
              gap: 6,
            }}
          >
            {liveSteps.map((step, i) => (
              <div
                key={`${step.step_order}-${step.status}-${step.attempts}-${i}`}
                className="card"
                style={{
                  padding: "10px 14px",
                  borderLeft: `3px solid ${statusColor(step.status)}`,
                  opacity: step.status === "corrected" ? 0.7 : 1,
                  transition: "all 0.3s ease",
                }}
              >
                <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                  <span style={{ fontSize: 16, minWidth: 24, textAlign: "center" }}>
                    {statusIcon(step.status)}
                  </span>
                  <div style={{ flex: 1 }}>
                    <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 2 }}>
                      <span style={{ fontWeight: 600, fontSize: 13 }}>Step {step.step_order}</span>
                      <span className="badge badge-muted" style={{ fontSize: 10 }}>{step.action_type}</span>
                      <span
                        style={{
                          fontSize: 11,
                          fontWeight: 600,
                          color: statusColor(step.status),
                          textTransform: "uppercase",
                        }}
                      >
                        {step.status}
                      </span>
                      {step.attempts > 1 && (
                        <span style={{ fontSize: 10, color: "var(--text-tertiary)" }}>
                          attempt {step.attempts}
                        </span>
                      )}
                    </div>
                    <div style={{ fontSize: 13, color: "var(--text-secondary, #ccc)" }}>{step.description}</div>
                    {step.target && (
                      <div style={{ fontSize: 11, color: "var(--text-tertiary)", fontFamily: "var(--mono)", marginTop: 2 }}>
                        target: {step.target}{step.value ? ` | value: ${step.value}` : ""}
                      </div>
                    )}
                    {step.llm_reasoning && (
                      <div
                        style={{
                          fontSize: 11,
                          color: "var(--text-tertiary)",
                          marginTop: 4,
                          padding: "4px 8px",
                          background: "rgba(255,255,255,0.04)",
                          borderRadius: 4,
                          fontStyle: "italic",
                        }}
                      >
                        AI: {step.llm_reasoning}
                      </div>
                    )}
                    {step.error && (
                      <div style={{ fontSize: 12, color: "var(--danger, #ef4444)", marginTop: 4 }}>{step.error}</div>
                    )}
                  </div>
                </div>
              </div>
            ))}

            {smartRunning && (
              <div className="card" style={{ padding: "10px 14px", textAlign: "center" }}>
                <div className="discover-dot" style={{ display: "inline-block", marginRight: 8 }} />
                <span style={{ fontSize: 13, color: "var(--text-tertiary)" }}>
                  AI is analyzing and executing steps...
                </span>
              </div>
            )}
          </div>
        </div>
      )}

      {/* Raw Task Actions — what the user actually did */}
      {automation && automation.raw_actions.length > 0 && liveSteps.length === 0 && (
        <div style={{ marginBottom: 24 }}>
          <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 10 }}>
            <h3 className="section-title" style={{ margin: 0 }}>
              Recorded User Actions ({automation.raw_actions.length})
            </h3>
            {automation.has_cached_plan && (
              <span className="badge badge-success" style={{ fontSize: 10 }}>
                Cached plan available
              </span>
            )}
          </div>
          <div
            className="card-inset"
            style={{ maxHeight: 300, overflow: "auto", padding: 0 }}
          >
            {automation.raw_actions.map((action, i) => {
              const [kind, ...rest] = action.split(":");
              const payload = rest.join(":");
              return (
                <div
                  key={i}
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: 8,
                    padding: "6px 12px",
                    borderBottom: "1px solid rgba(255,255,255,0.05)",
                    fontSize: 12,
                  }}
                >
                  <span style={{ fontWeight: 600, fontSize: 10, minWidth: 20, color: "var(--text-tertiary)" }}>
                    {i + 1}
                  </span>
                  <span
                    className="badge badge-muted"
                    style={{ fontSize: 10, minWidth: 60, textAlign: "center" }}
                  >
                    {kind}
                  </span>
                  <span style={{ fontFamily: "var(--mono)", color: "var(--text-secondary, #ccc)" }}>
                    {payload || "\u2014"}
                  </span>
                </div>
              );
            })}
          </div>
          <div style={{ fontSize: 11, color: "var(--text-tertiary)", marginTop: 6 }}>
            This is the raw data sent to the AI. It will understand the intent and create the automation from scratch.
            {automation.has_cached_plan && " A previously successful plan will be reused (instant, no LLM call)."}
          </div>
        </div>
      )}

      {/* Edit Steps */}
      <div>
        <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 14 }}>
          <h3 className="section-title" style={{ margin: 0 }}>
            Steps ({editedSteps.length})
          </h3>
          <button className="btn-primary btn-sm" onClick={onSaveSteps} disabled={isRunning}>
            Save Changes
          </button>
        </div>

        {!automation ? (
          <div style={{ display: "grid", gap: 10 }}>
            {[1, 2, 3].map((i) => (
              <div key={i} className="skeleton" style={{ height: 80, animationDelay: `${i * 100}ms` }} />
            ))}
          </div>
        ) : editedSteps.length === 0 ? (
          <div className="empty-state">
            <h3>No steps</h3>
            <p>This automation has no steps yet.</p>
          </div>
        ) : (
          <div className="task-list">
            {editedSteps.map((s, idx) => (
              <div
                key={s.step_id}
                className="step-card"
                style={{ animationDelay: `${idx * 60}ms` }}
              >
                <div className="step-number">{s.step_order}</div>
                <div className="step-content">
                  <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 10, marginBottom: 8 }}>
                    <div>
                      <label>Description</label>
                      <input
                        value={s.description}
                        onChange={(e) => {
                          const v = e.target.value;
                          setEditedSteps((prev) =>
                            prev.map((x) => (x.step_id === s.step_id ? { ...x, description: v } : x)),
                          );
                        }}
                      />
                    </div>
                    <div>
                      <label>Value</label>
                      <input
                        value={s.value}
                        onChange={(e) => {
                          const v = e.target.value;
                          setEditedSteps((prev) =>
                            prev.map((x) => (x.step_id === s.step_id ? { ...x, value: v } : x)),
                          );
                        }}
                      />
                    </div>
                  </div>
                  <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                    <span className="badge badge-muted">{s.action_type}</span>
                    <button
                      className="btn-danger btn-sm"
                      onClick={() => onDeleteStep(s.step_id)}
                      disabled={isRunning || editedSteps.length <= 1}
                      style={{ marginLeft: "auto" }}
                    >
                      Delete
                    </button>
                  </div>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
