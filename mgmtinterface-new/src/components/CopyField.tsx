import {useState, type ReactNode} from 'react'
import {CheckCircleIcon, CopyIcon} from './ui/icons'

/** Copy `text` to the clipboard and flash `true` for `ms`. */
export function useCopy(ms = 1500): [boolean, (text: string) => void] {
    const [copied, setCopied] = useState(false)

    function copy(text: string) {
        void navigator.clipboard?.writeText(text)
        setCopied(true)
        window.setTimeout(() => setCopied(false), ms)
    }

    return [copied, copy]
}

/** Icon-only button that copies `value` and briefly shows a check. */
export function CopyButton({
                               value,
                               label,
                               className = '',
                           }: {
    value: string
    label: string
    className?: string
}) {
    const [copied, copy] = useCopy()
    return (
        <button
            type="button"
            title={`Copy ${label}`}
            aria-label={`Copy ${label}`}
            onClick={() => copy(value)}
            className={`inline-flex h-7 w-7 shrink-0 items-center justify-center rounded-md text-fg-faint transition-colors hover:bg-surface-muted hover:text-fg-muted ${className}`}
        >
            {copied ? (
                <CheckCircleIcon className="h-3.5 w-3.5 text-success"/>
            ) : (
                <CopyIcon className="h-3.5 w-3.5"/>
            )}
        </button>
    )
}

/**
 * A labelled, read-only monospace value with a copy button — used for the
 * connection strings and credentials on the Ingest Share screen. `hint` renders
 * a small helper line under the label.
 */
export function CopyField({
                              label,
                              value,
                              hint,
                          }: {
    label: string
    value: string
    hint?: ReactNode
}) {
    return (
        <div>
            <div className="mb-1 flex items-baseline justify-between gap-3">
                <label className="text-[11px] font-bold uppercase tracking-wide text-fg-faint">
                    {label}
                </label>
                {hint && <span className="text-[11px] text-fg-faint">{hint}</span>}
            </div>
            <div className="flex items-start gap-1.5 rounded-lg border border-default bg-surface-muted/40 pl-3 pr-1">
                <code className="flex-1 overflow-x-auto whitespace-nowrap pt-2 pb-3 font-mono text-[13px] text-fg">
                    {value}
                </code>
                <CopyButton value={value} label={label.toLowerCase()} className="mt-1"/>
            </div>
        </div>
    )
}
