import type {InputHTMLAttributes, ReactNode} from 'react'
import {useId} from 'react'

interface InputProps extends InputHTMLAttributes<HTMLInputElement> {
    label?: ReactNode
    /** Small helper line rendered under the field. */
    help?: ReactNode
}

export function Input({label, help, id, className = '', ...props}: InputProps) {
    const generatedId = useId()
    const inputId = id ?? generatedId
    return (
        <div>
            {label && (
                <label
                    htmlFor={inputId}
                    className="mb-1.5 block text-sm font-semibold text-fg-muted"
                >
                    {label}
                </label>
            )}
            <input
                id={inputId}
                className={
                    'w-full rounded-lg border border-default bg-surface px-3 py-2.5 text-sm text-fg ' +
                    'placeholder:text-fg-faint focus:border-primary focus:outline-none ' +
                    'focus:shadow-[0_0_0_3px_var(--ring)] ' +
                    className
                }
                {...props}
            />
            {help && <p className="mt-1.5 text-xs text-fg-faint">{help}</p>}
        </div>
    )
}
