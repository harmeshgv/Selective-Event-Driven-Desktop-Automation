export type TaskOut = {
  task_id: number;
  signature: string;
  name: string;
  frequency: number;
  last_used: string;
  steps: string[];
  confidence_score: number;
};

export type AutomationStepOut = {
  step_id: number;
  step_order: number;
  description: string;
  action_type: string;
  target: string;
  value: string;
  retry_count: number;
};

export type AutomationPlanOut = {
  automation_id: number;
  task_id: number;
  name: string;
  risk_level: string;
  plan_text: string;
  steps: AutomationStepOut[];
};

export type RunAutomationIn = {
  automation_id: number;
  preview: boolean;
  approved: boolean;
};

export type RunStepOut = {
  step_order: number;
  description: string;
  status: string;
  attempts: number;
  error: string;
};

export type RunAutomationOut = {
  automation_id: number;
  plan_name: string;
  preview: boolean;
  risk_level: string;
  status: string;
  steps: RunStepOut[];
  error: string;
};

export type LogOut = {
  id: number;
  timestamp: string;
  app: string;
  action: string;
  coordinates: string;
  text: string;
  screenshot_path: string;
};

export type ObserverSettingsOut = {
  tracking_enabled: boolean;
  privacy_mode: boolean;
  screenshots_enabled: boolean;
  screenshot_every_seconds: number;
};

export type ObserverResetOut = {
  settings: ObserverSettingsOut;
  deleted_logs: number;
  deleted_tasks: number;
  deleted_task_steps: number;
  deleted_automations: number;
  deleted_automation_steps: number;
  deleted_runs: number;
  deleted_run_steps: number;
  deleted_screenshots: number;
};

async function getJSON<T>(path: string): Promise<T> {
  const resp = await fetch(path, { method: "GET" });
  if (!resp.ok) {
    const text = await resp.text();
    throw new Error(`GET ${path} failed: ${resp.status} ${text}`);
  }
  return (await resp.json()) as T;
}

async function postJSON<TReq, TResp>(path: string, body: TReq): Promise<TResp> {
  const resp = await fetch(path, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  if (!resp.ok) {
    const text = await resp.text();
    throw new Error(`POST ${path} failed: ${resp.status} ${text}`);
  }
  return (await resp.json()) as TResp;
}

async function putJSON<TReq, TResp>(path: string, body: TReq): Promise<TResp> {
  const resp = await fetch(path, {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  if (!resp.ok) {
    const text = await resp.text();
    throw new Error(`PUT ${path} failed: ${resp.status} ${text}`);
  }
  return (await resp.json()) as TResp;
}

async function deleteJSON<TReq, TResp>(path: string, body: TReq): Promise<TResp> {
  const resp = await fetch(path, {
    method: "DELETE",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  if (!resp.ok) {
    const text = await resp.text();
    throw new Error(`DELETE ${path} failed: ${resp.status} ${text}`);
  }
  return (await resp.json()) as TResp;
}

export function getTasks(): Promise<TaskOut[]> {
  return getJSON<TaskOut[]>("/tasks");
}

export function getAutomations(): Promise<AutomationPlanOut[]> {
  return getJSON<AutomationPlanOut[]>("/automations");
}

export function getLogs(limit: number = 200): Promise<LogOut[]> {
  return getJSON<LogOut[]>(`/logs?limit=${encodeURIComponent(String(limit))}`);
}

export function getObserverSettings(): Promise<ObserverSettingsOut> {
  return getJSON<ObserverSettingsOut>("/observer/settings");
}

export function updateObserverSettings(input: ObserverSettingsOut): Promise<ObserverSettingsOut> {
  // input shape matches server (includes screenshot_every_seconds).
  return putJSON<ObserverSettingsOut, ObserverSettingsOut>("/observer/settings", input);
}

export function resetObserverWorkspace(): Promise<ObserverResetOut> {
  return postJSON<Record<string, never>, ObserverResetOut>("/observer/reset", {});
}

export function clearLogs(confirm: boolean, limit: number): Promise<{ deleted: number }> {
  return deleteJSON<{ confirm: boolean; limit: number }, { deleted: number }>("/logs", { confirm, limit });
}

export function runAutomation(input: RunAutomationIn): Promise<RunAutomationOut> {
  return postJSON<RunAutomationIn, RunAutomationOut>("/run", input);
}

export type UpdateAutomationStepIn = {
  step_id: number;
  step_order: number;
  description: string;
  action_type: string;
  target: string;
  value: string;
  retry_count: number;
};

export function updateAutomationSteps(
  automationId: number,
  steps: UpdateAutomationStepIn[],
): Promise<AutomationPlanOut> {
  return putJSON<{ steps: UpdateAutomationStepIn[] }, AutomationPlanOut>(`/automations/${automationId}/steps`, {
    steps,
  });
}

export function createAutomation(taskId: number): Promise<AutomationPlanOut> {
  return postJSON<{ task_id: number }, AutomationPlanOut>("/automations", { task_id: taskId });
}

