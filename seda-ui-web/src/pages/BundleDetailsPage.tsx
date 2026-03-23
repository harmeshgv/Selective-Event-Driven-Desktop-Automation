import { useEffect, useMemo, useState } from 'react'
import { api } from '../lib/api'
import type { AutomationPlan, PlanStep, RepeatedTaskBundle } from '../lib/api'

function prettify(v: unknown) {
  try {
    return JSON.stringify(v, null, 2)
  } catch {
    return String(v)
  }
}

export function BundleDetailsPage() {
  const [minFreq, setMinFreq] = useState(2)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [bundles, setBundles] = useState<RepeatedTaskBundle[]>([])
  const [bundle, setBundle] = useState<RepeatedTaskBundle | null>(null)
  const [actionIdx, setActionIdx] = useState<number | null>(null)
  const [showRaw, setShowRaw] = useState(false)

  const [planLoading, setPlanLoading] = useState(false)
  const [planError, setPlanError] = useState<string | null>(null)
  const [plan, setPlan] = useState<AutomationPlan | null>(null)
  const [stepIdx, setStepIdx] = useState<number | null>(null)
  const [showRawSteps, setShowRawSteps] = useState(false)

  const actions = bundle?.sample_run ?? []
  const action = useMemo(() => {
    if (actionIdx == null) return null
    return actions[actionIdx] ?? null
  }, [actions, actionIdx])

  async function load() {
    setLoading(true)
    setError(null)
    setBundle(null)
    setActionIdx(null)
    try {
      const res = await api.repeatedTasks({ min_frequency: minFreq, limit: 25, flow_limit: 5000 })
      setBundles(res.data ?? [])
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    void load()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  useEffect(() => {
    setShowRaw(false)
  }, [bundle, actionIdx])

  useEffect(() => {
    setPlan(null)
    setPlanError(null)
    setStepIdx(null)
    setShowRawSteps(false)
    if (!bundle) return
    setPlanLoading(true)
    void api
      .resolvePlan(bundle)
      .then((res) => {
        setPlan(res.data ?? null)
        setStepIdx(0)
      })
      .catch((e) => setPlanError(e instanceof Error ? e.message : String(e)))
      .finally(() => setPlanLoading(false))
  }, [bundle])

  const steps = plan?.plan_steps ?? []
  const step: PlanStep | null = useMemo(() => {
    if (stepIdx == null) return null
    return steps[stepIdx] ?? null
  }, [steps, stepIdx])

  function setStep(next: PlanStep) {
    if (!plan) return
    const nextSteps = [...(plan.plan_steps ?? [])]
    const idx = stepIdx ?? 0
    nextSteps[idx] = next
    setPlan({ ...plan, plan_steps: nextSteps })
  }

  async function savePlan() {
    if (!plan) return
    setPlanLoading(true)
    setPlanError(null)
    try {
      const res = await api.savePlan(plan.pattern_hash, {
        plan_steps: plan.plan_steps,
        source_last_seen_ms: plan.source_last_seen_ms,
        plan_version: plan.plan_version ?? 1,
      })
      setPlan(res.data ?? plan)
    } catch (e) {
      setPlanError(e instanceof Error ? e.message : String(e))
    } finally {
      setPlanLoading(false)
    }
  }

  function moveStep(dir: -1 | 1) {
    if (!plan) return
    const idx = stepIdx ?? 0
    const j = idx + dir
    if (j < 0 || j >= steps.length) return
    const nextSteps = [...steps]
    const tmp = nextSteps[idx]
    nextSteps[idx] = nextSteps[j]
    nextSteps[j] = tmp
    setPlan({ ...plan, plan_steps: nextSteps })
    setStepIdx(j)
  }

  function deleteStep() {
    if (!plan) return
    const idx = stepIdx ?? 0
    const nextSteps = steps.filter((_, i) => i !== idx)
    setPlan({ ...plan, plan_steps: nextSteps })
    setStepIdx(Math.max(0, Math.min(idx, nextSteps.length - 1)))
  }

  function addStepAfter() {
    if (!plan) return
    const idx = stepIdx ?? (steps.length - 1)
    const newStep: PlanStep = {
      id: `u${Date.now()}`,
      kind: 'click',
      title: 'New step',
      app: bundle?.sequence_label ? null : null,
      domain: null,
      payload: {},
      warnings: [],
      safety: { destructive: false, requires_confirmation: false },
    }
    const nextSteps = [...steps.slice(0, idx + 1), newStep, ...steps.slice(idx + 1)]
    setPlan({ ...plan, plan_steps: nextSteps })
    setStepIdx(idx + 1)
  }

  return (
    <div className="space-y-6">
      <div className="flex items-start justify-between gap-6">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">Bundle details</h1>
          <p className="subtle mt-1">Inspect actions and details for a representative run.</p>
        </div>
        <div className="flex items-center gap-2">
          <input className="input w-[120px]" type="number" min={2} max={64} value={minFreq} onChange={(e) => setMinFreq(Number(e.target.value))} />
          <button className="btn btn-primary" onClick={() => void load()}>
            {loading ? 'Loading…' : 'Reload'}
          </button>
        </div>
      </div>

      {error ? (
        <div className="rounded-lg border border-red-500/30 bg-red-500/10 p-3 text-sm text-red-600 dark:text-red-300">
          {error}
        </div>
      ) : null}

      <div className="grid gap-4 lg:grid-cols-12">
        <div className="lg:col-span-4">
          <div className="mb-2 font-medium">Bundles</div>
          <div className="card overflow-hidden">
            <div className="max-h-[540px] overflow-auto">
              {bundles.length === 0 ? (
                <div className="p-6 text-sm text-[rgb(var(--muted))]">No bundles yet.</div>
              ) : (
                <ul className="divide-y divide-[rgb(var(--border))]">
                  {bundles.map((b) => {
                    const active = bundle?.pattern_hash === b.pattern_hash
                    return (
                      <li key={b.pattern_hash}>
                        <button
                          className={[
                            'w-full p-4 text-left transition',
                            active ? 'bg-black/5 dark:bg-white/10' : 'hover:bg-black/5 dark:hover:bg-white/10',
                          ].join(' ')}
                          onClick={() => {
                            setBundle(b)
                            setActionIdx(null)
                          }}
                        >
                          <div className="font-medium">{(b.sequence?.length ?? 0)} steps · x{b.frequency}</div>
                          <div className="subtle mt-1 line-clamp-2">{b.sequence_label}</div>
                        </button>
                      </li>
                    )
                  })}
                </ul>
              )}
            </div>
          </div>
        </div>

        <div className="lg:col-span-3 space-y-4">
          <div>
            <div className="mb-2 flex items-center justify-between">
              <div className="font-medium">Plan steps (editable)</div>
              <div className="flex items-center gap-2">
                <button className="btn" onClick={() => setShowRawSteps((v) => !v)} disabled={!bundle}>
                  {showRawSteps ? 'Hide raw' : 'Show raw'}
                </button>
                <button className="btn btn-primary" onClick={() => void savePlan()} disabled={!plan || planLoading}>
                  {planLoading ? 'Saving…' : 'Save'}
                </button>
              </div>
            </div>
            <div className="card overflow-hidden">
              <div className="max-h-[360px] overflow-auto">
                {!bundle ? (
                  <div className="p-6 text-sm text-[rgb(var(--muted))]">Select a bundle.</div>
                ) : planLoading ? (
                  <div className="p-6 text-sm text-[rgb(var(--muted))]">Loading plan…</div>
                ) : planError ? (
                  <div className="p-4 text-sm text-red-600 dark:text-red-300">{planError}</div>
                ) : steps.length === 0 ? (
                  <div className="p-6 text-sm text-[rgb(var(--muted))]">No plan steps yet.</div>
                ) : (
                  <ul className="divide-y divide-[rgb(var(--border))]">
                    {steps.map((s, idx) => {
                      const active = stepIdx === idx
                      const warn = (s.warnings?.length ?? 0) > 0
                      return (
                        <li key={s.id ?? idx}>
                          <button
                            className={[
                              'w-full p-3 text-left transition',
                              active ? 'bg-black/5 dark:bg-white/10' : 'hover:bg-black/5 dark:hover:bg-white/10',
                            ].join(' ')}
                            onClick={() => setStepIdx(idx)}
                          >
                            <div className="flex items-center justify-between gap-2">
                              <div className="text-sm font-medium">
                                {idx + 1}. {s.title}
                              </div>
                              {warn ? <div className="text-xs rounded-md border border-amber-500/30 bg-amber-500/10 px-2 py-0.5">warn</div> : null}
                            </div>
                            <div className="subtle mt-0.5 text-xs">
                              {s.kind}
                              {s.app ? ` · ${s.app}` : ''}
                            </div>
                          </button>
                        </li>
                      )
                    })}
                  </ul>
                )}
              </div>
            </div>

            {showRawSteps ? (
              <div className="mt-3">
                <div className="subtle mb-1 text-xs">Raw `automation_steps`</div>
                <pre className="max-h-[220px] overflow-auto rounded-lg border border-[rgb(var(--border))] bg-black/5 p-3 text-xs dark:bg-white/10">
                  {prettify(bundle?.automation_steps ?? [])}
                </pre>
              </div>
            ) : null}
          </div>

          <div>
            <div className="mb-2 font-medium">Edit step</div>
            <div className="card p-4 space-y-3">
              {!step ? (
                <div className="text-sm text-[rgb(var(--muted))]">Select a plan step.</div>
              ) : (
                <>
                  {step.warnings?.length ? (
                    <div className="rounded-lg border border-amber-500/30 bg-amber-500/10 p-3 text-xs">
                      <div className="font-medium mb-1">Warnings</div>
                      <ul className="list-disc pl-5 space-y-1">
                        {step.warnings.map((w, i) => (
                          <li key={i}>
                            <span className="font-mono">{w.code}</span> — {w.message}
                          </li>
                        ))}
                      </ul>
                    </div>
                  ) : null}

                  <div className="grid gap-2">
                    <div className="subtle">Title</div>
                    <input className="input" value={step.title ?? ''} onChange={(e) => setStep({ ...step, title: e.target.value })} />
                  </div>
                  <div className="grid gap-2">
                    <div className="subtle">Kind</div>
                    <input className="input" value={step.kind ?? ''} onChange={(e) => setStep({ ...step, kind: e.target.value })} />
                  </div>
                  <div className="grid gap-2">
                    <div className="subtle">App</div>
                    <input className="input" value={step.app ?? ''} onChange={(e) => setStep({ ...step, app: e.target.value })} />
                  </div>
                  <div className="grid gap-2">
                    <div className="subtle">Domain/URL</div>
                    <input className="input" value={step.domain ?? ''} onChange={(e) => setStep({ ...step, domain: e.target.value })} />
                  </div>
                  <div className="grid gap-2">
                    <div className="subtle">Payload (JSON)</div>
                    <textarea
                      className="input min-h-[140px] font-mono text-xs"
                      value={prettify(step.payload ?? {})}
                      onChange={(e) => {
                        try {
                          const next = JSON.parse(e.target.value || '{}') as Record<string, unknown>
                          setStep({ ...step, payload: next })
                        } catch {
                          // keep text until valid JSON; do nothing
                        }
                      }}
                    />
                    <div className="subtle text-xs">Edit selector/url/query/etc here. (Must be valid JSON to apply.)</div>
                  </div>

                  <div className="flex flex-wrap items-center gap-2 pt-2">
                    <button className="btn" onClick={() => moveStep(-1)} disabled={(stepIdx ?? 0) <= 0}>
                      Move up
                    </button>
                    <button className="btn" onClick={() => moveStep(1)} disabled={(stepIdx ?? 0) >= steps.length - 1}>
                      Move down
                    </button>
                    <button className="btn" onClick={() => addStepAfter()}>
                      Add after
                    </button>
                    <button className="btn" onClick={() => deleteStep()} disabled={steps.length <= 1}>
                      Delete
                    </button>
                  </div>
                </>
              )}
            </div>
          </div>
        </div>

        <div className="lg:col-span-5">
          <div className="mb-2 font-medium">Details</div>
          <div className="card p-4">
            {!action ? (
              <div className="text-sm text-[rgb(var(--muted))]">Select an action to see details.</div>
            ) : (
              <div className="space-y-4">
                <div className="grid gap-3">
                  <div className="grid grid-cols-3 gap-2 border-b border-[rgb(var(--border))] pb-3">
                    <div className="subtle">Action</div>
                    <div className="col-span-2 text-sm font-medium">{String((action as any).action_type ?? '—')}</div>
                    <div className="subtle">App</div>
                    <div className="col-span-2 text-sm">{String((action as any).target_app ?? (action as any).source_app ?? '—')}</div>
                    <div className="subtle">Domain</div>
                    <div className="col-span-2 text-sm">{String((action as any).website_domain ?? '—')}</div>
                    <div className="subtle">Query</div>
                    <div className="col-span-2 text-sm">{String((action as any).search_query ?? '—')}</div>
                    <div className="subtle">Timestamp</div>
                    <div className="col-span-2 text-sm font-mono text-xs">{String((action as any).timestamp_iso ?? '—')}</div>
                  </div>

                  <div className="flex items-center justify-between">
                    <div className="text-sm font-medium">Raw JSON</div>
                    <button className="btn" onClick={() => setShowRaw((v) => !v)}>
                      {showRaw ? 'Hide' : 'Show'}
                    </button>
                  </div>

                  {showRaw ? (
                    <pre className="max-h-[360px] overflow-auto rounded-lg border border-[rgb(var(--border))] bg-black/5 p-3 text-xs dark:bg-white/10">
                      {prettify(action)}
                    </pre>
                  ) : null}
                </div>
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  )
}

