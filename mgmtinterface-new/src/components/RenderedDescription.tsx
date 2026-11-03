import {lazy, Suspense} from 'react'

// The Markdown renderer is about a third of the app's size, and only a page
// that shows a description needs it, so it loads on first use.
const Markdown = lazy(() =>
    import('./Markdown').then((module) => ({default: module.Markdown})),
)

/**
 * A description rendered from Markdown, as on the project page, in a session's
 * dialog and above a folder in the data browser. The text as it is until the
 * renderer has loaded.
 */
export function RenderedDescription({text}: { text: string }) {
    return (
        <Suspense
            fallback={
                <p className="whitespace-pre-wrap text-sm leading-relaxed text-fg wrap-anywhere">
                    {text}
                </p>
            }
        >
            <Markdown>{text}</Markdown>
        </Suspense>
    )
}
