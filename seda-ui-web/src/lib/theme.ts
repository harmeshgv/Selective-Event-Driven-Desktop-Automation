type ThemeMode = 'light' | 'dark' | 'system'

const KEY = 'seda.theme'

export function getTheme(): ThemeMode {
  const raw = localStorage.getItem(KEY)
  if (raw === 'light' || raw === 'dark' || raw === 'system') return raw
  return 'system'
}

export function setTheme(mode: ThemeMode) {
  localStorage.setItem(KEY, mode)
  applyTheme(mode)
}

export function initTheme() {
  applyTheme(getTheme())
}

export function applyTheme(mode: ThemeMode) {
  const root = document.documentElement
  const prefersDark = window.matchMedia?.('(prefers-color-scheme: dark)').matches
  const useDark = mode === 'dark' || (mode === 'system' && prefersDark)
  root.classList.toggle('dark', useDark)
}

