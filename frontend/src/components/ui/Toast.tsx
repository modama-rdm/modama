import { useCallback, useMemo, useRef, useState, type ReactNode } from 'react'
import { ToastContext, type ToastType } from './toast-context'

interface ToastItem {
  id: number
  type: ToastType
  message: string
}

const AUTO_DISMISS_MS = 3500

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<ToastItem[]>([])
  const nextId = useRef(1)

  const notify = useCallback((message: string, type: ToastType = 'success') => {
    const id = nextId.current++
    setToasts((prev) => [...prev, { id, type, message }])
    setTimeout(() => {
      setToasts((prev) => prev.filter((t) => t.id !== id))
    }, AUTO_DISMISS_MS)
  }, [])

  const api = useMemo(() => ({ notify }), [notify])

  return (
    <ToastContext.Provider value={api}>
      {children}
      <div className="pointer-events-none fixed bottom-6 right-6 z-[9999] flex flex-col gap-2">
        {toasts.map((t) => (
          <div
            key={t.id}
            className="flex min-w-[280px] items-start gap-3 rounded-xl border-l-4 bg-surface px-4 py-3.5 shadow-lg [animation:toastIn_0.22s_ease-out]"
            style={{ borderLeftColor: t.type === 'success' ? 'var(--success)' : 'var(--danger)' }}
          >
            <span className="shrink-0 text-lg leading-none">
              {t.type === 'success' ? '✅' : '❌'}
            </span>
            <div>
              <div className="text-sm font-bold text-fg">
                {t.type === 'success' ? 'Done!' : 'Error'}
              </div>
              <div className="mt-0.5 text-xs leading-snug text-fg-muted">
                {t.message}
              </div>
            </div>
          </div>
        ))}
      </div>
    </ToastContext.Provider>
  )
}
