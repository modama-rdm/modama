import {useEffect, useId, useRef, type ReactNode} from 'react'
import {XIcon} from './icons'

/**
 * A modal dialog: the native `<dialog>`, opened with `showModal()` once it is
 * mounted. The browser keeps focus inside it, makes the page behind inert and
 * closes it on Escape; it sits in the top layer, so no card or table clips it.
 * The close button and a click on the dimmed backdrop close it too. Every way
 * of closing ends in `onClose`, where the parent unmounts it.
 */
export function Dialog({
                           title,
                           subtitle,
                           actions,
                           onClose,
                           children,
                       }: {
    title: ReactNode
    subtitle?: ReactNode
    /** Buttons left of the close button, such as Edit. */
    actions?: ReactNode
    onClose: () => void
    children: ReactNode
}) {
    const ref = useRef<HTMLDialogElement>(null)
    /** Whether the press that ends in a click began on the backdrop. */
    const pressedOnBackdrop = useRef(false)
    const titleId = useId()

    useEffect(() => {
        // Guarded: StrictMode runs this twice on mount.
        if (ref.current && !ref.current.open) ref.current.showModal()
    }, [])

    return (
        <dialog
            ref={ref}
            aria-labelledby={titleId}
            onClose={onClose}
            // The panel fills the dialog, so a press on the dialog itself is
            // one on the backdrop. Both ends of the click count: selecting
            // text in the panel and letting go outside it is no click away.
            onPointerDown={(e) => {
                pressedOnBackdrop.current = e.target === e.currentTarget
            }}
            onClick={(e) => {
                if (pressedOnBackdrop.current && e.target === e.currentTarget) e.currentTarget.close()
            }}
            className="m-auto max-h-[85vh] w-[min(44rem,calc(100vw-2rem))] rounded-2xl border border-default bg-surface p-0 text-fg shadow-lg backdrop:bg-scrim"
        >
            <div className="flex max-h-[85vh] flex-col">
                <div className="flex items-start justify-between gap-4 border-b border-default px-5 py-4">
                    <div className="min-w-0">
                        <h2 id={titleId} className="text-[15px] font-semibold text-fg wrap-anywhere">
                            {title}
                        </h2>
                        {subtitle && <div className="mt-0.5 text-xs text-fg-faint">{subtitle}</div>}
                    </div>
                    <div className="flex shrink-0 items-center gap-2">
                        {actions}
                        <button
                            type="button"
                            aria-label="Close"
                            onClick={() => ref.current?.close()}
                            className="rounded p-1 text-fg-faint hover:bg-surface-muted hover:text-fg"
                        >
                            <XIcon className="h-4 w-4"/>
                        </button>
                    </div>
                </div>
                <div className="overflow-y-auto px-5 py-4">{children}</div>
            </div>
        </dialog>
    )
}
