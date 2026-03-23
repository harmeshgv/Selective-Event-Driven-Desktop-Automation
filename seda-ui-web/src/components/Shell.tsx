import { getTheme, setTheme } from '../lib/theme'
import { useMemo, useState } from 'react'

function ThemeToggle() {
  const [mode, setMode] = useState(() => getTheme())

  return (
    <div className="flex items-center gap-2">
      <span className="subtle">Theme</span>
      <select
        className="input w-[140px] py-2"
        value={mode}
        onChange={(e) => {
          const v = e.target.value as 'light' | 'dark' | 'system'
          setMode(v)
          setTheme(v)
        }}
      >
        <option value="system">System</option>
        <option value="light">Light</option>
        <option value="dark">Dark</option>
      </select>
    </div>
  )
}

export function Shell({ children }: { children: React.ReactNode }) {
  const year = useMemo(() => new Date().getFullYear(), [])
  return (
    <div className="min-h-full bg-[rgb(var(--bg))]">
      <div className="mx-auto flex min-h-screen max-w-[1200px] gap-6 p-6">
        <main className="flex-1">
          <header className="mb-6 flex items-center justify-between">
            <div>
              <div className="text-xl font-semibold tracking-tight">SEDA</div>
              <div className="subtle">Local desktop automation mining</div>
            </div>
            <ThemeToggle />
          </header>

          <div className="card p-6">{children}</div>
          <div className="mt-4 text-xs text-[rgb(var(--muted))]">© {year} SEDA</div>
        </main>
      </div>
    </div>
  )
}

