export type TaskOut = {
  task_id: number;
  signature: string;
  name: string;
  frequency: number;
  last_used: string;
  steps: string[];
  confidence_score: number;
};

export type ExplainTaskIn = {
  task_id?: number;
  task_name?: string;
  signature?: string;
  actions: string[];
  repeat_count: number;
  last_used?: string;
  confidence_score?: number;
};

export type ExplainTaskOut = {
  explanation: string;
  provider: string;
  cached: boolean;
  used_fallback: boolean;
  is_repeated: boolean;
  repeated_confidence: number;
  repeated_reason: string;
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
  const resp = await fetch(path, { method: "GET", cache: "no-store" });
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

export type GetTasksOptions = {
  limitLogs?: number;
  /** When true, runs full pattern discovery over logs (slower). When false, reads persisted tasks from DB (fast). */
  discover?: boolean;
};

export function getTasks(options: GetTasksOptions = {}): Promise<TaskOut[]> {
  const params = new URLSearchParams();
  if (options.limitLogs != null) {
    params.set("limit_logs", String(options.limitLogs));
  }
  if (options.discover === true) {
    params.set("discover", "true");
  } else if (options.discover === false) {
    params.set("discover", "false");
  }
  const suffix = params.size > 0 ? `?${params.toString()}` : "";
  return getJSON<TaskOut[]>(`/tasks${suffix}`);
}

export function explainTask(input: ExplainTaskIn): Promise<ExplainTaskOut> {
  return postJSON<ExplainTaskIn, ExplainTaskOut>("/tasks/explain", input);
}

export type GetAutomationsOptions = {
  taskId?: number;
  limit?: number;
};

export function getAutomations(options: GetAutomationsOptions = {}): Promise<AutomationPlanOut[]> {
  const params = new URLSearchParams();
  if (options.taskId != null) {
    params.set("task_id", String(options.taskId));
  }
  if (options.limit != null) {
    params.set("limit", String(options.limit));
  }
  const suffix = params.size > 0 ? `?${params.toString()}` : "";
  return getJSON<AutomationPlanOut[]>(`/automations${suffix}`);
}

export function getAutomation(automationId: number): Promise<AutomationPlanOut> {
  return getJSON<AutomationPlanOut>(`/automations/${automationId}`);
}

export type GetLogsOptions = {
  limit?: number;
  sinceId?: number;
};

export function getLogs(options: GetLogsOptions = {}): Promise<LogOut[]> {
  const params = new URLSearchParams();
  params.set("limit", String(options.limit ?? 200));
  if (options.sinceId != null) {
    params.set("since_id", String(options.sinceId));
  }
  return getJSON<LogOut[]>(`/logs?${params.toString()}`);
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

