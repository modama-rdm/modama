import type {ReactNode} from 'react'
import {descriptionDigest} from '../lib/format'

/**
 * A description's first line under a name in a list row, cut to one
 * line, with the whole text on hover. Nothing when there is no description.
 * The hover is the native `title`, like every other hover in the app.
 * `children` follow the line, such as a button that shows it all.
 */
export function DescriptionDigest({
                                      description,
                                      className = '',
                                      children,
                                  }: {
    description?: string | null
    className?: string
    children?: ReactNode
}) {
    const digest = descriptionDigest(description)
    if (!digest) return null
    return (
        <div className={`mt-0.5 flex items-center gap-1.5 text-xs text-fg-muted ${className}`}>
            <span className="min-w-0 truncate" title={description ?? undefined}>
                {digest}
            </span>
            {children}
        </div>
    )
}
