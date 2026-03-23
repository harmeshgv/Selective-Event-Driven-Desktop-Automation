import { useEffect, useMemo, useState } from 'react'
import { api } from '../lib/api'
import type { CollectorSnapshot } from '../lib/api'

function formatStatus(s: CollectorSnapshot | null) {
  if (!s) return { label: 'Unknown', pill: 'Unknown' }
  if (s.collecting) return { label: 'Collecting', pill: `Collecting (${(s.session_id ?? '').slice(0, 8)})` }
  return { label: 'Idle', pill: 'Idle' }
}

export function SessionPage() {
  const [loading, setLoading] = useState(true)
  const [snapshot, setSnapshot] = useState<CollectorSnapshot | null>(null)
  const [error, setError] = useState<string | null>(null)

  const status = useMemo(() => formatStatus(snapshot), [snapshot])

  async function refresh() {
    setError(null)
    try {
      const res = await api.status()
      setSnapshot(res.data)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    void refresh()
    const id = window.setInterval(() => void refresh(), 2500)
    return () => window.clearInterval(id)
  }, [])

  async function run(op: 'start' | 'stop' | 'clear') {
    setError(null)
    try {
      const res = await (op === 'start' ? api.start() : op === 'stop' ? api.stop() : api.clear())
      setSnapshot(res.data)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }

  return (
    <div className="space-y-6">
      <div className="flex items-start justify-between gap-6">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">Session</h1>
          <p className="subtle mt-1">Start collecting, stop to finalize, clear to reset local data.</p>
        </div>
        <div className="rounded-full border border-[rgb(var(--border))] px-3 py-1 text-sm">
          {loading ? 'Loading…' : status.pill}
        </div>
      </div>

      <div className="flex flex-wrap gap-2">
        <button className="btn btn-primary" onClick={() => void run('start')} disabled={!!snapshot?.collecting}>
          Start session
        </button>
        <button className="btn btn-primary" onClick={() => void run('stop')} disabled={!snapshot?.collecting}>
          Stop session
        </button>
        <button className="btn" onClick={() => void refresh()}>
          Refresh
        </button>
        <button className="btn btn-danger" onClick={() => void run('clear')}>
          Clear data
        </button>
      </div>

      <div className="card p-4">
        <div className="flex items-center justify-between">
          <div className="font-medium">Collector status</div>
          <div className="subtle">{status.label}</div>
        </div>
        <div className="mt-3 grid gap-2 text-sm">
          <div className="flex justify-between border-t border-[rgb(var(--border))] pt-2">
            <div className="subtle">Session ID</div>
            <div className="font-mono text-xs">{snapshot?.session_id ?? '—'}</div>
          </div>
          <div className="flex justify-between border-t border-[rgb(var(--border))] pt-2">
            <div className="subtle">Started (ms)</div>
            <div className="font-mono text-xs">{snapshot?.started_ms ?? '—'}</div>
          </div>
          <div className="flex justify-between border-t border-[rgb(var(--border))] pt-2">
            <div className="subtle">Action count</div>
            <div className="font-mono text-xs">{snapshot?.action_count ?? '—'}</div>
          </div>
        </div>
        {error ? (
          <div className="mt-3 rounded-lg border border-red-500/30 bg-red-500/10 p-3 text-sm text-red-600 dark:text-red-300">
            {error}
          </div>
        ) : null}
      </div>
    </div>
  )
}

