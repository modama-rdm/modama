import { useState } from 'react'

/**
 * PR1 placeholder. This screen exists only to prove the toolchain end-to-end:
 * Tailwind v4 utilities resolving to the CSS-variable design tokens, the fonts,
 * the branding asset, and the light/dark theme swap. The real app shell, router,
 * and login screen arrive in later PRs.
 */
function App() {
  const [dark, setDark] = useState(false)

  function toggleTheme() {
    const next = !dark
    setDark(next)
    document.documentElement.classList.toggle('dark', next)
  }

  return (
    <div className="min-h-screen bg-bg flex items-center justify-center p-6">
      <div className="w-full max-w-md rounded-2xl border border-default bg-surface shadow-sm p-10 text-center">
        <img src="/branding/modama.svg" alt="modama" className="h-9 mx-auto" />
        <h1 className="mt-4 text-lg font-bold text-fg">Data Management</h1>
        <p className="mt-1 text-sm text-fg-subtle">ER-C Self-Service Portal</p>

        <p className="mt-6 text-sm text-fg-muted leading-relaxed">
          Frontend scaffold is live. Design tokens, Tailwind, fonts and theming
          are wired up — screens land in the next PRs.
        </p>

        <code className="mt-6 block text-xs font-mono text-fg-faint">
          npm run dev · {dark ? 'dark' : 'light'} theme
        </code>

        <button
          onClick={toggleTheme}
          className="mt-6 rounded-lg bg-primary px-5 py-2.5 text-sm font-semibold text-primary-fg hover:bg-primary-hover transition-colors"
        >
          Toggle {dark ? 'light' : 'dark'} theme
        </button>
      </div>
    </div>
  )
}

export default App
