import { useMemo, useState } from 'react'
import { api } from '../lib/api'
import type { BundleExplanation, RepeatedTaskBundle } from '../lib/api'

export function RepeatedTasksPage() {
  const [minFreq, setMinFreq] = useState(2)
  const [limit, setLimit] = useState(25)
  const [query, setQuery] = useState('')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [bundles, setBundles] = useState<RepeatedTaskBundle[]>([])
  const [selected, setSelected] = useState<RepeatedTaskBundle | null>(null)
  const [explainFor, setExplainFor] = useState<RepeatedTaskBundle | null>(null)
  const [explainLoading, setExplainLoading] = useState(false)
  const [explainError, setExplainError] = useState<string | null>(null)
  const [explainText, setExplainText] = useState<string | null>(null)
  const [explainMeta, setExplainMeta] = useState<{ provider: string; model: string } | null>(null)

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase()
    if (!q) return bundles
    return bundles.filter((b) => (b.sequence_label ?? '').toLowerCase().includes(q))
  }, [bundles, query])

  async function load() {
    setLoading(true)
    setError(null)
    setSelected(null)
    try {
      const res = await api.repeatedTasks({ min_frequency: minFreq, limit, flow_limit: 5000 })
      setBundles(res.data ?? [])
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setLoading(false)
    }
  }

  async function explain(bundle: RepeatedTaskBundle) {
    setExplainFor(bundle)
    setExplainLoading(true)
    setExplainError(null)
    setExplainText(null)
    setExplainMeta(null)
    try {
      const res = await api.explainBundle(bundle)
      const data = res.data as BundleExplanation
      if (!data.enabled || !data.explanation) {
        throw new Error(data.error || 'Explanation unavailable (LLM disabled).')
      }
      setExplainMeta({ provider: data.provider, model: data.model })
      setExplainText(data.explanation)
    } catch (e) {
      setExplainError(e instanceof Error ? e.message : String(e))
    } finally {
      setExplainLoading(false)
    }
  }

  return (
    <div className="space-y-6">
      <div className="flex items-start justify-between gap-6">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">Repeated tasks</h1>
          <p className="subtle mt-1">Mine your recent actions to find workflows you repeat.</p>
        </div>
        <button className="btn btn-primary" onClick={() => void load()}>
          {loading ? 'Loading…' : 'Load'}
        </button>
      </div>

      <div className="grid gap-3 md:grid-cols-3">
        <div className="space-y-1">
          <div className="subtle">Min repeats</div>
          <input className="input" type="number" min={2} max={64} value={minFreq} onChange={(e) => setMinFreq(Number(e.target.value))} />
        </div>
        <div className="space-y-1">
          <div className="subtle">Limit</div>
          <input className="input" type="number" min={1} max={100} value={limit} onChange={(e) => setLimit(Number(e.target.value))} />
        </div>
        <div className="space-y-1">
          <div className="subtle">Search</div>
          <input className="input" value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Filter by sequence label…" />
        </div>
      </div>

      {error ? (
        <div className="rounded-lg border border-red-500/30 bg-red-500/10 p-3 text-sm text-red-600 dark:text-red-300">
          {error}
        </div>
      ) : null}

      <div className="grid gap-4 lg:grid-cols-5">
        <div className="lg:col-span-3">
          <div className="mb-2 flex items-center justify-between">
            <div className="font-medium">Bundles</div>
            <div className="subtle">{filtered.length} items</div>
          </div>
          <div className="card overflow-hidden">
            <div className="max-h-[520px] overflow-auto">
              {filtered.length === 0 ? (
                <div className="p-6 text-sm text-[rgb(var(--muted))]">
                  No bundles yet. Start a session and repeat a workflow 2+ times, then load again.
                </div>
              ) : (
                <ul className="divide-y divide-[rgb(var(--border))]">
                  {filtered.map((b) => {
                    const steps = b.sequence?.length ?? 0
                    const isActive = selected?.pattern_hash === b.pattern_hash
                    return (
                      <li key={b.pattern_hash}>
                        <div
                          className={[
                            'p-4 transition',
                            isActive ? 'bg-black/5 dark:bg-white/10' : 'hover:bg-black/5 dark:hover:bg-white/10',
                          ].join(' ')}
                        >
                          <button className="block w-full text-left" onClick={() => setSelected(b)}>
                            <div className="flex items-center justify-between gap-4">
                              <div className="font-medium">
                                {steps} steps · x{b.frequency}
                              </div>
                              <div className="subtle">{b.last_seen_iso ? b.last_seen_iso.slice(0, 10) : ''}</div>
                            </div>
                            <div className="subtle mt-1 line-clamp-2">{b.sequence_label}</div>
                          </button>
                          <div className="mt-3 flex items-center gap-2">
                            <button className="btn" onClick={() => void explain(b)}>
                              Explain
                            </button>
                          </div>
                        </div>
                      </li>
                    )
                  })}
                </ul>
              )}
            </div>
          </div>
        </div>

        <div className="lg:col-span-2">
          <div className="mb-2 font-medium">Preview</div>
          <div className="card p-4">
            {!selected ? (
              <div className="text-sm text-[rgb(var(--muted))]">Select a bundle to preview its sample run.</div>
            ) : (
              <div className="space-y-3">
                <div>
                  <div className="text-sm font-medium">Sequence</div>
                  <div className="subtle mt-1 line-clamp-3">{selected.sequence_label}</div>
                </div>
                <div className="border-t border-[rgb(var(--border))] pt-3">
                  <div className="text-sm font-medium">Sample run</div>
                  <div className="subtle mt-1">
                    {(selected.sample_run?.length ?? 0) === 0
                      ? 'No sample run available.'
                      : `${selected.sample_run?.length ?? 0} actions captured.`}
                  </div>
                </div>
                <div className="border-t border-[rgb(var(--border))] pt-3">
                  <div className="text-sm font-medium">Tip</div>
                  <div className="subtle mt-1">
                    Open <span className="font-medium text-[rgb(var(--fg))]">Bundle details</span> to inspect actions + raw JSON.
                  </div>
                </div>
              </div>
            )}
          </div>
        </div>
      </div>

      {explainFor ? (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4">
          <div className="card w-full max-w-3xl p-5">
            <div className="flex items-start justify-between gap-4">
              <div>
                <div className="text-lg font-semibold tracking-tight">Explanation</div>
                <div className="subtle mt-1 line-clamp-2">{explainFor.sequence_label}</div>
                {explainMeta ? (
                  <div className="subtle mt-1 text-xs">
                    {explainMeta.provider} · {explainMeta.model}
                  </div>
                ) : null}
              </div>
              <button
                className="btn"
                onClick={() => {
                  setExplainFor(null)
                  setExplainError(null)
                  setExplainText(null)
                  setExplainMeta(null)
                }}
              >
                Close
              </button>
            </div>

            <div className="mt-4">
              {explainLoading ? (
                <div className="subtle">Asking the LLM…</div>
              ) : explainError ? (
                <div className="rounded-lg border border-red-500/30 bg-red-500/10 p-3 text-sm text-red-600 dark:text-red-300">
                  {explainError}
                  <div className="subtle mt-2 text-xs">
                    To enable: set <span className="font-mono">SEDA_LLM_PROVIDER=ollama</span> (and run Ollama) or{' '}
                    <span className="font-mono">SEDA_LLM_PROVIDER=groq</span> with <span className="font-mono">SEDA_GROQ_API_KEY</span>.
                  </div>
                </div>
              ) : (
                <pre className="max-h-[420px] overflow-auto rounded-lg border border-[rgb(var(--border))] bg-black/5 p-3 text-xs dark:bg-white/10 whitespace-pre-wrap">
                  {explainText ?? ''}
                </pre>
              )}
            </div>
          </div>
        </div>
      ) : null}
    </div>
  )
}

