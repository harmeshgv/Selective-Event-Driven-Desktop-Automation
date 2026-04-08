import React, { useEffect, useState } from "react";
import { useParams } from "react-router-dom";
import {
  getAutomations,
  type AutomationStepOut,
  runAutomation,
  updateAutomationSteps,
  type AutomationPlanOut,
  type RunAutomationOut,
} from "../api";

export default function AutomationDetail() {
  const { automationId } = useParams();
  const id = Number(automationId);

  const [automation, setAutomation] = useState<AutomationPlanOut | null>(null);
  const [editedSteps, setEditedSteps] = useState<AutomationStepOut[]>([]);
  const [preview, setPreview] = useState<RunAutomationOut | null>(null);
  const [approved, setApproved] = useState(false);
  const [error, setError] = useState<string>("");
  const [running, setRunning] = useState(false);

  useEffect(() => {
    (async () => {
      try {
        const all = await getAutomations();
        const found = all.find((x) => x.automation_id === id) ?? null;
        setAutomation(found);
        setEditedSteps(found?.steps ?? []);
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      }
    })();
  }, [id]);

  useEffect(() => {
    if (!id) return;
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
      await updateAutomationSteps(
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
      // After edits, require explicit user approval again.
      setApproved(false);
      await refreshPreview();
      // Keep automation state in sync.
      const all = await getAutomations();
      const found = all.find((x) => x.automation_id === id) ?? null;
      setAutomation(found);
      setEditedSteps(found?.steps ?? []);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setRunning(false);
    }
  }

  async function onExecute() {
    if (!automation) return;
    setRunning(true);
    setError("");
    try {
      const res = await runAutomation({ automation_id: id, preview: false, approved: approved });
      setPreview(res);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setRunning(false);
    }
  }

  return (
    <div style={{ padding: 16 }}>
      <h1 style={{ marginTop: 0 }}>Automation</h1>

      {error ? (
        <pre style={{ color: "crimson", whiteSpace: "pre-wrap" }}>{error}</pre>
      ) : null}

      <div style={{ marginBottom: 12 }}>
        <strong>ID:</strong> {id}
        <br />
        <strong>Risk:</strong> {preview?.risk_level ?? automation?.risk_level ?? "unknown"}
      </div>

      <div style={{ marginBottom: 12 }}>
        <label>
          <input
            type="checkbox"
            checked={approved}
            onChange={(e) => setApproved(e.target.checked)}
            disabled={running}
          />
          I approve execution
        </label>
      </div>

      <button onClick={onExecute} disabled={!approved || running}>
        {running ? "Running..." : "Execute (approval required)"}
      </button>

      <section style={{ marginTop: 24 }}>
        <h2>Step-by-step plan</h2>
        <pre style={{ background: "#f6f6f6", padding: 12, overflow: "auto" }}>
          {preview ? JSON.stringify(preview.steps, null, 2) : "Loading preview..."}
        </pre>
      </section>

      <section style={{ marginTop: 24 }}>
        <h2>Edit Steps</h2>
        {automation ? (
          <>
            {editedSteps.map((s) => (
              <div
                key={s.step_id}
                style={{
                  border: "1px solid #ddd",
                  borderRadius: 8,
                  padding: 12,
                  marginBottom: 12,
                }}
              >
                <div style={{ marginBottom: 8 }}>
                  <strong>Step {s.step_order}</strong> ({s.action_type})
                </div>
                <div style={{ marginBottom: 8 }}>
                  <label>
                    Description{" "}
                    <input
                      style={{ width: 420, maxWidth: "100%" }}
                      value={s.description}
                      onChange={(e) => {
                        const v = e.target.value;
                        setEditedSteps((prev) =>
                          prev.map((x) => (x.step_id === s.step_id ? { ...x, description: v } : x)),
                        );
                      }}
                    />
                  </label>
                </div>
                <div style={{ marginBottom: 8 }}>
                  <label>
                    Value{" "}
                    <input
                      style={{ width: 420, maxWidth: "100%" }}
                      value={s.value}
                      onChange={(e) => {
                        const v = e.target.value;
                        setEditedSteps((prev) =>
                          prev.map((x) => (x.step_id === s.step_id ? { ...x, value: v } : x)),
                        );
                      }}
                    />
                  </label>
                </div>
              </div>
            ))}

            <button onClick={onSaveSteps} disabled={running}>
              Save steps
            </button>
          </>
        ) : (
          <div>Loading automation...</div>
        )}
      </section>
    </div>
  );
}

