import {useCallback, useEffect, useState} from 'react'

export type Theme = 'light' | 'dark'

const STORAGE_KEY = 'erc-theme'

/**
 * Storage can be unavailable (private mode, disabled, sandboxed) and then both
 * reading and writing throw. The theme is a preference, not state we can't live
 * without, so degrade to the OS setting rather than take `AppShell` — and with it
 * every authenticated screen — down on render. Mirrors `tokenStore` in api/auth.ts.
 */
function getInitialTheme(): Theme {
    try {
        const stored = localStorage.getItem(STORAGE_KEY)
        if (stored === 'light' || stored === 'dark') return stored
    } catch {
        /* storage unavailable — fall through to the OS preference */
    }
    return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'
}

export function useTheme() {
    const [theme, setTheme] = useState<Theme>(getInitialTheme)

    useEffect(() => {
        document.documentElement.classList.toggle('dark', theme === 'dark')
        try {
            localStorage.setItem(STORAGE_KEY, theme)
        } catch {
            /* storage unavailable — the toggle still works for this session */
        }
    }, [theme])

    const toggleTheme = useCallback(() => {
        setTheme(t => (t === 'dark' ? 'light' : 'dark'))
    }, [])

    return {theme, toggleTheme}
}
