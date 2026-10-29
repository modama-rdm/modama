import {descriptionDigest} from '../lib/format'

/**
 * A description's first line under a name in a list row (#112), cut to one
 * line, with the whole text on hover. Nothing when there is no description.
 * The hover is the native `title`, like every other hover in the app.
 */
export function DescriptionDigest({
                                      description,
                                      className = '',
                                  }: {
    description?: string | null
    className?: string
}) {
    const digest = descriptionDigest(description)
    if (!digest) return null
    return (
        <div
            className={`mt-0.5 truncate text-xs text-fg-muted ${className}`}
            title={description ?? undefined}
        >
            {digest}
        </div>
    )
}
