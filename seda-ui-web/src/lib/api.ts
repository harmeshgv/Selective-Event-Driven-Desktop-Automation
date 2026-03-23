export type ApiEnvelope<T> = {
  success: boolean
  message: string
  data: T
}

export type CollectorSnapshot = {
  collecting: boolean
  session_id: string | null
  started_ms: number | null
  action_count: number | null
  message?: string | null
}

export type RepeatedTaskBundle = {
  pattern_hash: string
  sequence: string[]
  sequence_label: string
  frequency: number
  avg_duration_ms: number | null
  last_seen_iso?: string | null
  last_seen_ms?: number | null
  sample_run?: Record<string, unknown>[]
  automation_steps?: Record<string, unknown>[]
  plan_steps?: PlanStep[]
}

export type PlanWarning = {
  code: string
  message: string
}

export type PlanStep = {
  id: string
  kind: string
  title: string
  app?: string | null
  domain?: string | null
  payload: Record<string, unknown>
  source_step_ids?: string[]
  warnings?: PlanWarning[]
  safety?: { destructive?: boolean; requires_confirmation?: boolean }
}

export type AutomationPlan = {
  pattern_hash: string
  created_ms: number
  updated_ms: number
  plan_version: number
  source_last_seen_ms: number | null
  plan_steps: PlanStep[]
}

export type BundleExplanation = {
  enabled: boolean
  provider: string
  model: string
  explanation: string | null
  error: string | null
}

const BASE = import.meta.env.VITE_SEDA_API_BASE ?? ''

async function http<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${BASE}${path}`, {
    ...init,
    headers: {
      'Content-Type': 'application/json',
      ...(init?.headers ?? {}),
    },
  })
  if (!res.ok) {
    const text = await res.text().catch(() => '')
    throw new Error(`${res.status} ${res.statusText}${text ? ` — ${text}` : ''}`)
  }
  return (await res.json()) as T
}

export const api = {
  health: () => fetch(`${BASE}/health`).then((r) => r.ok),
  status: () => http<ApiEnvelope<CollectorSnapshot>>(`/api/dashboard/status`),
  start: () => http<ApiEnvelope<CollectorSnapshot>>(`/api/dashboard/start`, { method: 'POST' }),
  stop: () => http<ApiEnvelope<CollectorSnapshot>>(`/api/dashboard/stop`, { method: 'POST' }),
  clear: () => http<ApiEnvelope<CollectorSnapshot>>(`/api/dashboard/clear`, { method: 'POST' }),
  repeatedTasks: (params: { min_frequency: number; limit: number; flow_limit: number }) => {
    const q = new URLSearchParams({
      min_frequency: String(params.min_frequency),
      limit: String(params.limit),
      flow_limit: String(params.flow_limit),
    })
    return http<ApiEnvelope<RepeatedTaskBundle[]>>(`/api/dashboard/repeated_tasks?${q}`)
  },
  explainBundle: (bundle: RepeatedTaskBundle) =>
    http<ApiEnvelope<BundleExplanation>>(`/api/ui/explain_bundle`, {
      method: 'POST',
      body: JSON.stringify(bundle),
    }),
  resolvePlan: (bundle: RepeatedTaskBundle) =>
    http<ApiEnvelope<AutomationPlan>>(`/api/plans/resolve`, {
      method: 'POST',
      body: JSON.stringify(bundle),
    }),
  savePlan: (pattern_hash: string, plan: Pick<AutomationPlan, 'plan_steps' | 'source_last_seen_ms' | 'plan_version'>) =>
    http<ApiEnvelope<AutomationPlan>>(`/api/plans/${encodeURIComponent(pattern_hash)}`, {
      method: 'PUT',
      body: JSON.stringify(plan),
    }),
}

