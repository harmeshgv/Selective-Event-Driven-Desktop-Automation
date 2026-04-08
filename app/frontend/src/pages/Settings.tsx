import React, { useEffect, useState } from "react";
import type { ObserverSettingsOut } from "../api";
import { clearLogs, getObserverSettings, resetObserverWorkspace, updateObserverSettings } from "../api";

function summarizeCollection(s: ObserverSettingsOut) {
  const enabled: string[] = [];
  if (s.tracking_enabled) {
    enabled.push("Mouse clicks/moves + keyboard");
  } else {
    enabled.push("Paused (no events sent)");
  }
  if (s.privacy_mode) {
    enabled.push("Privacy mode (typed text masked; screenshots skipped)");
  } else if (s.screenshots_enabled) {
    enabled.push(`Screenshots enabled (${s.screenshot_every_seconds}s interval)`);
  } else {
    enabled.push("Screenshots disabled");
  }
  return enabled;
}

export default function Settings() {
  const [original, setOriginal] = useState<ObserverSettingsOut | null>(null);
  const [draft, setDraft] = useState<ObserverSettingsOut | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string>("");
  const [loading, setLoading] = useState(false);
  const [notice, setNotice] = useState<string>("");

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

  useEffect(() => {
    load();
  }, []);

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
    await saveSettings(draft, "Settings saved.");
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
    if (
      !window.confirm(
        "Reset everything? This will stop recording and permanently delete logs, tasks, automations, runs, and screenshots.",
      )
    ) {
      return;
    }
    setSaving(true);
    setError("");
    setNotice("");
    try {
      const res = await resetObserverWorkspace();
      setOriginal(res.settings);
      setDraft({ ...res.settings });
      setNotice(
        `Workspace reset. Deleted ${res.deleted_logs} logs, ${res.deleted_tasks} tasks, ${res.deleted_automations} automations, and ${res.deleted_screenshots} screenshots.`,
      );
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  }

  async function onPanic() {
    if (!draft) return;
    await saveSettings(
      {
        ...draft,
        privacy_mode: true,
        screenshots_enabled: false,
      },
      "Privacy mode enabled.",
    );
  }

  async function onStart() {
    if (!draft) return;
    await saveSettings(
      {
        ...draft,
        tracking_enabled: true,
      },
      "Recording started.",
    );
  }

  async function onStop() {
    if (!draft) return;
    await saveSettings(
      {
        ...draft,
        tracking_enabled: false,
      },
      "Recording stopped.",
    );
  }

  return (
    <div style={{ padding: 16 }}>
      <h1 style={{ marginTop: 0 }}>Tracking & Privacy</h1>
      <p style={{ marginTop: 8, color: "#555", maxWidth: 760 }}>
        Start should start, stop should stop, and reset should clean the workspace. The primary controls below apply
        immediately.
      </p>

      {error ? <pre style={{ color: "crimson", whiteSpace: "pre-wrap" }}>{error}</pre> : null}
      {notice ? (
        <div
          style={{
            marginBottom: 16,
            padding: 12,
            borderRadius: 10,
            background: "#eef8ef",
            border: "1px solid #c8e6cb",
            color: "#155724",
          }}
        >
          {notice}
        </div>
      ) : null}

      {loading || !draft ? (
        <div>Loading...</div>
      ) : (
        <>
          <section
            style={{
              border: "1px solid #eee",
              borderRadius: 12,
              padding: 16,
              marginBottom: 16,
              background: draft.tracking_enabled ? "#f7fff8" : "#fff8f8",
            }}
          >
            <h2 style={{ marginTop: 0, marginBottom: 8 }}>Recorder</h2>
            <div style={{ fontSize: 18, fontWeight: 700, marginBottom: 6 }}>
              {draft.tracking_enabled ? "Recording is on" : "Recording is off"}
            </div>
            <div style={{ color: "#555", marginBottom: 14 }}>
              {draft.privacy_mode ? "Privacy mode is on." : "Privacy mode is off."}{" "}
              {draft.screenshots_enabled ? `Screenshots every ${draft.screenshot_every_seconds}s.` : "Screenshots are off."}
            </div>

            <div style={{ display: "flex", gap: 12, flexWrap: "wrap" }}>
              <button onClick={onStart} disabled={saving || draft.tracking_enabled}>
                {saving && !draft.tracking_enabled ? "Starting..." : "Start"}
              </button>
              <button onClick={onStop} disabled={saving || !draft.tracking_enabled}>
                {saving && draft.tracking_enabled ? "Stopping..." : "Stop"}
              </button>
              <button onClick={onPanic} disabled={saving || draft.privacy_mode}>
                Privacy mode
              </button>
              <button onClick={onResetEverything} disabled={saving}>
                Reset everything
              </button>
            </div>
          </section>

          <section style={{ border: "1px solid #eee", borderRadius: 10, padding: 12, marginBottom: 16 }}>
            <h2 style={{ marginTop: 0 }}>Current behavior</h2>
            <ul>
              {summary.map((x, i) => (
                <li key={i}>{x}</li>
              ))}
            </ul>
          </section>

          <section style={{ border: "1px solid #eee", borderRadius: 10, padding: 12, marginBottom: 16 }}>
            <h2 style={{ marginTop: 0 }}>Advanced settings</h2>
            <p style={{ marginTop: 0, color: "#555" }}>Edit these values, then save when you are ready.</p>

            <div style={{ marginBottom: 12 }}>
              <label>
                <input
                  type="checkbox"
                  checked={draft.tracking_enabled}
                  onChange={(e) => setDraft({ ...draft, tracking_enabled: e.target.checked })}
                  disabled={saving}
                />{" "}
                Enable event recording (sent to backend)
              </label>
            </div>

            <div style={{ marginBottom: 12 }}>
              <label>
                <input
                  type="checkbox"
                  checked={draft.privacy_mode}
                  onChange={(e) =>
                    setDraft({
                      ...draft,
                      privacy_mode: e.target.checked,
                      // In this MVP, privacy mode also implies skipping screenshots.
                      screenshots_enabled: e.target.checked ? false : draft.screenshots_enabled,
                    })
                  }
                  disabled={saving}
                />{" "}
                Privacy mode (mask typed text; skip screenshots)
              </label>
            </div>

            <div style={{ marginBottom: 12, opacity: draft.privacy_mode ? 0.6 : 1 }}>
              <label>
                <input
                  type="checkbox"
                  checked={draft.screenshots_enabled}
                  onChange={(e) => setDraft({ ...draft, screenshots_enabled: e.target.checked })}
                  disabled={saving || draft.privacy_mode}
                />{" "}
                Capture screenshots
              </label>
            </div>

            <div style={{ marginBottom: 12, opacity: draft.screenshots_enabled && !draft.privacy_mode ? 1 : 0.6 }}>
              <label>
                Screenshot interval (seconds):{" "}
                <input
                  type="number"
                  min={5}
                  max={120}
                  value={draft.screenshot_every_seconds}
                  onChange={(e) => setDraft({ ...draft, screenshot_every_seconds: Number(e.target.value) })}
                  disabled={saving || !draft.screenshots_enabled || draft.privacy_mode}
                />
              </label>
            </div>
          </section>

          <button onClick={onSave} disabled={saving}>
            {saving ? "Saving..." : "Save settings"}
          </button>
          <button
            style={{ marginLeft: 12 }}
            onClick={() => (original ? setDraft({ ...original }) : null)}
            disabled={saving || !original || !dirty}
          >
            Discard edits
          </button>

          <section style={{ border: "1px solid #eee", borderRadius: 10, padding: 12, marginTop: 16 }}>
            <h2 style={{ marginTop: 0 }}>Maintenance</h2>
            <p style={{ marginTop: 0, color: "#555" }}>
              Use this if you want to clear recent event history without resetting everything else.
            </p>
            <button onClick={onClearLogs} disabled={saving}>
              Clear recent logs
            </button>
          </section>
        </>
      )}
    </div>
  );
}

