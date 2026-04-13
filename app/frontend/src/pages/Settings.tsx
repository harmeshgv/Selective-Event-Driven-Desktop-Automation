import React, { useEffect, useState } from "react";
import type { ObserverSettingsOut } from "../api";
import { clearLogs, getObserverSettings, resetObserverWorkspace, updateObserverSettings } from "../api";

function summarizeCollection(s: ObserverSettingsOut) {
  const items: string[] = [];
  if (s.tracking_enabled) items.push("Mouse clicks/moves + keyboard");
  else items.push("Paused (no events sent)");
  if (s.privacy_mode) items.push("Privacy mode (text masked, no screenshots)");
  else if (s.screenshots_enabled) items.push(`Screenshots every ${s.screenshot_every_seconds}s`);
  else items.push("Screenshots off");
  return items;
}

export default function Settings() {
  const [original, setOriginal] = useState<ObserverSettingsOut | null>(null);
  const [draft, setDraft] = useState<ObserverSettingsOut | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const [notice, setNotice] = useState("");

  const dirty =
    !!original &&
    !!draft &&
    (original.tracking_enabled !== draft.tracking_enabled ||
      original.privacy_mode !== draft.privacy_mode ||
      original.screenshots_enabled !== draft.screenshots_enabled ||
      original.screenshot_every_seconds !== draft.screenshot_every_seconds);

  function normalizeSettings(input: ObserverSettingsOut): ObserverSettingsOut {
    const nextInterval = Number.isFinite(input.screenshot_every_seconds)
      ? Math.max(5, Math.min(3600, Math.round(input.screenshot_every_seconds)))
      : 30;
    return {
      tracking_enabled: input.tracking_enabled,
      privacy_mode: input.privacy_mode,
      screenshots_enabled: input.privacy_mode ? false : input.screenshots_enabled,
      screenshot_every_seconds: nextInterval,
    };
  }

  async function load() {
    setLoading(true);
    setError("");
    setNotice("");
    try {
      const s = await getObserverSettings();
      setOriginal(s);
      setDraft({ ...s });
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => { load(); }, []);

  const summary = draft ? summarizeCollection(draft) : [];

  async function saveSettings(next: ObserverSettingsOut, successMessage: string) {
    setSaving(true);
    setError("");
    setNotice("");
    try {
      const updated = await updateObserverSettings(normalizeSettings(next));
      setOriginal(updated);
      setDraft({ ...updated });
      setNotice(successMessage);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  }

  async function onSave() {
    if (!draft) return;
    await saveSettings(draft, "Settings saved successfully.");
  }

  async function onClearLogs() {
    if (!window.confirm("Delete the last 2000 recorded events?")) return;
    setSaving(true);
    setError("");
    setNotice("");
    try {
      const res = await clearLogs(true, 2000);
      setNotice(`Deleted ${res.deleted} recent log(s).`);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  }

  async function onResetEverything() {
    if (!window.confirm("Reset everything? This will stop recording and permanently delete all data.")) return;
    setSaving(true);
    setError("");
    setNotice("");
    try {
      const res = await resetObserverWorkspace();
      setOriginal(res.settings);
      setDraft({ ...res.settings });
      setNotice(`Workspace reset. Deleted ${res.deleted_logs} logs, ${res.deleted_tasks} tasks, ${res.deleted_automations} automations, and ${res.deleted_screenshots} screenshots.`);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  }

  async function onPanic() {
    if (!draft) return;
    await saveSettings({ ...draft, privacy_mode: true, screenshots_enabled: false }, "Privacy mode enabled.");
  }

  async function onStart() {
    if (!draft) return;
    await saveSettings({ ...draft, tracking_enabled: true }, "Recording started.");
  }

  async function onStop() {
    if (!draft) return;
    await saveSettings({ ...draft, tracking_enabled: false }, "Recording stopped.");
  }

  return (
    <div>
      <div className="page-header">
        <h1>Settings</h1>
        <p>Control recording behavior, privacy, and data management.</p>
      </div>

      {error && <div className="error-banner">{error}</div>}
      {notice && <div className="notice-banner">{notice}</div>}

      {loading || !draft ? (
        <div style={{ display: "grid", gap: 16 }}>
          {[1, 2, 3].map((i) => (
            <div key={i} className="skeleton" style={{ height: 120, animationDelay: `${i * 100}ms` }} />
          ))}
        </div>
      ) : (
        <>
          {/* Recorder Status */}
          <div className="card" style={{ marginBottom: 16 }}>
            <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 14 }}>
              <div>
                <h3 className="section-title" style={{ margin: 0 }}>Recorder</h3>
                <span style={{ fontSize: 13, color: "var(--text-secondary)" }}>
                  {draft.privacy_mode ? "Privacy mode active" : draft.tracking_enabled ? "Recording events" : "Paused"}
                </span>
              </div>
              <span
                className={`badge ${draft.tracking_enabled ? "badge-success" : "badge-muted"}`}
                style={{ fontSize: 12 }}
              >
                {draft.tracking_enabled ? "Active" : "Inactive"}
              </span>
            </div>

            <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
              <button className="btn-primary btn-sm" onClick={onStart} disabled={saving || draft.tracking_enabled}>
                Start
              </button>
              <button className="btn-sm" onClick={onStop} disabled={saving || !draft.tracking_enabled}>
                Stop
              </button>
              <button className="btn-sm" onClick={onPanic} disabled={saving || draft.privacy_mode}>
                🔒 Privacy Mode
              </button>
            </div>
          </div>

          {/* Current Behavior */}
          <div className="card" style={{ marginBottom: 16 }}>
            <h3 className="section-title" style={{ marginBottom: 10 }}>Current Behavior</h3>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              {summary.map((x, i) => (
                <div key={i} style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 13 }}>
                  <span style={{ width: 6, height: 6, borderRadius: "50%", background: "var(--accent)", flexShrink: 0 }} />
                  <span style={{ color: "var(--text-secondary)" }}>{x}</span>
                </div>
              ))}
            </div>
          </div>

          {/* Advanced */}
          <div className="card" style={{ marginBottom: 16 }}>
            <h3 className="section-title" style={{ marginBottom: 4 }}>Advanced</h3>
            <p style={{ fontSize: 13, color: "var(--text-tertiary)", margin: "0 0 16px" }}>
              Edit these values and save when ready.
            </p>

            <div className="toggle-row">
              <div className="toggle-label">
                <span>Event Recording</span>
                <span>Send mouse and keyboard events to backend</span>
              </div>
              <button
                className="toggle-switch"
                data-on={String(draft.tracking_enabled)}
                onClick={() => setDraft({ ...draft, tracking_enabled: !draft.tracking_enabled })}
                disabled={saving}
                type="button"
              />
            </div>

            <div className="toggle-row">
              <div className="toggle-label">
                <span>Privacy Mode</span>
                <span>Mask typed text and skip screenshots</span>
              </div>
              <button
                className="toggle-switch"
                data-on={String(draft.privacy_mode)}
                onClick={() =>
                  setDraft({
                    ...draft,
                    privacy_mode: !draft.privacy_mode,
                    screenshots_enabled: !draft.privacy_mode ? false : draft.screenshots_enabled,
                  })
                }
                disabled={saving}
                type="button"
              />
            </div>

            <div className="toggle-row" style={{ opacity: draft.privacy_mode ? 0.4 : 1 }}>
              <div className="toggle-label">
                <span>Screenshots</span>
                <span>Capture periodic screenshots</span>
              </div>
              <button
                className="toggle-switch"
                data-on={String(draft.screenshots_enabled)}
                onClick={() => setDraft({ ...draft, screenshots_enabled: !draft.screenshots_enabled })}
                disabled={saving || draft.privacy_mode}
                type="button"
              />
            </div>

            <div
              className="toggle-row"
              style={{ opacity: draft.screenshots_enabled && !draft.privacy_mode ? 1 : 0.4 }}
            >
              <div className="toggle-label">
                <span>Screenshot Interval</span>
                <span>Seconds between captures (5–120)</span>
              </div>
              <input
                type="number"
                min={5}
                max={120}
                value={draft.screenshot_every_seconds}
                onChange={(e) => setDraft({ ...draft, screenshot_every_seconds: Number(e.target.value) })}
                disabled={saving || !draft.screenshots_enabled || draft.privacy_mode}
                style={{ width: 80, textAlign: "center" }}
              />
            </div>

            <div style={{ display: "flex", gap: 8, marginTop: 16 }}>
              <button className="btn-primary btn-sm" onClick={onSave} disabled={saving}>
                {saving ? "Saving..." : "Save Settings"}
              </button>
              <button
                className="btn-sm"
                onClick={() => (original ? setDraft({ ...original }) : null)}
                disabled={saving || !dirty}
              >
                Discard
              </button>
            </div>
          </div>

          {/* Danger Zone */}
          <div
            className="card"
            style={{ borderColor: "var(--danger)", background: "var(--danger-bg)" }}
          >
            <h3 className="section-title" style={{ color: "var(--danger)", marginBottom: 4 }}>Danger Zone</h3>
            <p style={{ fontSize: 13, color: "var(--text-secondary)", margin: "0 0 14px" }}>
              These actions are destructive and cannot be undone.
            </p>
            <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
              <button className="btn-danger btn-sm" onClick={onClearLogs} disabled={saving}>
                Clear Recent Logs
              </button>
              <button className="btn-danger btn-sm" onClick={onResetEverything} disabled={saving}>
                Reset Everything
              </button>
            </div>
          </div>
        </>
      )}
    </div>
  );
}
