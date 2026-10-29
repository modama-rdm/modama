import type {ReactNode, TextareaHTMLAttributes} from 'react'
import {useId} from 'react'

interface TextareaProps extends TextareaHTMLAttributes<HTMLTextAreaElement> {
    label?: ReactNode
    /** Small helper line rendered under the field. */
    help?: ReactNode
}

/** A multi-line `Input`: the same label, help line and focus ring. */
export function Textarea({label, help, id, className = '', rows = 3, ...props}: TextareaProps) {
    const generatedId = useId()
    const textareaId = id ?? generatedId
    return (
        <div>
            {label && (
                <label
                    htmlFor={textareaId}
                    className="mb-1.5 block text-sm font-semibold text-fg-muted"
                >
                    {label}
                </label>
            )}
            <textarea
                id={textareaId}
                rows={rows}
                className={
                    'block min-h-24 w-full resize-y rounded-lg border border-default bg-surface px-3 py-2.5 ' +
                    'text-sm leading-relaxed text-fg placeholder:text-fg-faint focus:border-primary ' +
                    'focus:outline-none focus:shadow-[0_0_0_3px_var(--ring)] ' +
                    className
                }
                {...props}
            />
            {help && <p className="mt-1.5 text-xs text-fg-faint">{help}</p>}
        </div>
    )
}
