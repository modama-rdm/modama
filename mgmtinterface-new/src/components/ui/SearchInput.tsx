import type {InputHTMLAttributes} from 'react'
import {SearchIcon} from './icons'

/**
 * The search box above a table: the projects list's and a project's sessions'
 * (#113). `className` goes on the wrapper, for the margin; everything else is
 * the input's.
 */
export function SearchInput({className = '', ...props}: InputHTMLAttributes<HTMLInputElement>) {
    return (
        <div className={`relative ${className}`}>
            <SearchIcon
                className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-fg-faint"/>
            <input
                {...props}
                className="w-full rounded-lg border border-default bg-surface py-2.5 pl-9 pr-3 text-sm text-fg placeholder:text-fg-faint focus:border-primary focus:outline-none focus:shadow-[0_0_0_3px_var(--ring)]"
            />
        </div>
    )
}
