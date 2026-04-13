import React, { useEffect, useState } from "react";
import { Link, useParams } from "react-router-dom";
import {
  getAutomation,
  type AutomationStepOut,
  runAutomation,
  updateAutomationSteps,
  type AutomationPlanOut,
  type RunAutomationOut,
} from "../api";

function riskBadgeClass(risk: string) {
  const r = risk.toLowerCase();
  if (r === "low") return "badge badge-success";
  if (r === "high") return "badge badge-danger";
  return "badge badge-warning";
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

  const risk = preview?.risk_level ?? automation?.risk_level ?? "unknown";

  return (
    <div>
      {/* Breadcrumb */}
      <div style={{ marginBottom: 16 }}>
        <Link to="/" style={{ fontSize: 13, color: "var(--text-tertiary)" }}>
          ← Back to Dashboard
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
      <div className="card" style={{ marginBottom: 20, display: "flex", alignItems: "center", gap: 16 }}>
        <label style={{ display: "flex", alignItems: "center", gap: 8, cursor: "pointer" }}>
          <button
            className="toggle-switch"
            data-on={String(approved)}
            onClick={() => setApproved((v) => !v)}
            disabled={running}
            type="button"
          />
          <span style={{ fontSize: 14, fontWeight: 500 }}>I approve execution</span>
        </label>
        <button className="btn-primary" onClick={onExecute} disabled={!approved || running} style={{ marginLeft: "auto" }}>
          {running ? "Executing..." : "Execute Automation"}
        </button>
      </div>

      {/* Preview */}
      {preview && (
        <div style={{ marginBottom: 24 }}>
          <h3 className="section-title">Execution Preview</h3>
          <div className="card-inset" style={{ fontFamily: "var(--mono)", fontSize: 12, whiteSpace: "pre-wrap", maxHeight: 260, overflow: "auto" }}>
            {JSON.stringify(preview.steps, null, 2)}
          </div>
        </div>
      )}

      {/* Edit Steps */}
      <div>
        <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 14 }}>
          <h3 className="section-title" style={{ margin: 0 }}>
            Steps ({editedSteps.length})
          </h3>
          <button className="btn-primary btn-sm" onClick={onSaveSteps} disabled={running}>
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
          <div style={{ display: "grid", gap: 10 }}>
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
                      disabled={running || editedSteps.length <= 1}
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
