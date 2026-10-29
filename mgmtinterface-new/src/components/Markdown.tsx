import ReactMarkdown, {type Components} from 'react-markdown'
import remarkGfm from 'remark-gfm'

/**
 * Element styles for a rendered description (#112). No typography plugin: the
 * few elements a README uses are styled here, in the app's tokens.
 *
 * Headings are shifted down (`#` becomes an `<h3>`), so a description never
 * competes with the page's own `<h1>`. Images are shown as links instead of
 * being loaded: a README may point anywhere, and opening a project page should
 * not fetch from a third-party host.
 */
const components: Components = {
    h1: ({children}) => <h3 className="mt-4 mb-2 text-lg font-bold text-fg first:mt-0">{children}</h3>,
    h2: ({children}) => <h3 className="mt-4 mb-2 text-base font-bold text-fg first:mt-0">{children}</h3>,
    h3: ({children}) => <h4 className="mt-3 mb-1.5 text-sm font-bold text-fg first:mt-0">{children}</h4>,
    h4: ({children}) => <h5 className="mt-3 mb-1.5 text-sm font-semibold text-fg first:mt-0">{children}</h5>,
    h5: ({children}) => <h6 className="mt-3 mb-1.5 text-sm font-semibold text-fg-muted first:mt-0">{children}</h6>,
    h6: ({children}) => <h6 className="mt-3 mb-1.5 text-sm font-semibold text-fg-muted first:mt-0">{children}</h6>,
    p: ({children}) => <p className="my-2 first:mt-0 last:mb-0">{children}</p>,
    ul: ({children}) => <ul className="my-2 list-disc pl-5 first:mt-0 last:mb-0">{children}</ul>,
    ol: ({children}) => <ol className="my-2 list-decimal pl-5 first:mt-0 last:mb-0">{children}</ol>,
    li: ({children}) => <li className="my-0.5">{children}</li>,
    // An unsafe target (`javascript:` and the like) arrives as an empty href;
    // such a link is shown as its text.
    a: ({href, children}) => !href ? <span>{children}</span> : (
        <a
            href={href}
            target="_blank"
            rel="noopener noreferrer"
            className="text-primary underline decoration-primary/40 underline-offset-2 hover:decoration-primary"
        >
            {children}
        </a>
    ),
    img: ({src, alt}) =>
        typeof src === 'string' && src ? (
            <a
                href={src}
                target="_blank"
                rel="noopener noreferrer"
                className="text-primary underline decoration-primary/40 underline-offset-2 hover:decoration-primary"
            >
                {alt || src}
            </a>
        ) : null,
    blockquote: ({children}) => (
        <blockquote className="my-2 border-l-2 border-default pl-3 text-fg-muted">{children}</blockquote>
    ),
    code: ({children}) => (
        <code className="rounded bg-surface-muted px-1 py-0.5 font-mono text-[12px]">{children}</code>
    ),
    pre: ({children}) => (
        <pre
            className="my-2 overflow-x-auto rounded-lg bg-surface-muted p-3 font-mono text-[12px] leading-relaxed [&_code]:bg-transparent [&_code]:p-0">
            {children}
        </pre>
    ),
    hr: () => <hr className="my-3 border-default"/>,
    table: ({children}) => (
        <div className="my-2 overflow-x-auto">
            <table className="border-collapse text-sm">{children}</table>
        </div>
    ),
    th: ({children}) => (
        <th className="border border-default px-2.5 py-1.5 text-left font-semibold">{children}</th>
    ),
    td: ({children}) => <td className="border border-default px-2.5 py-1.5">{children}</td>,
}

/**
 * A description rendered from Markdown, with GitHub's extensions (tables, task
 * lists, strikethrough, autolinks). Raw HTML in the text is shown as text, not
 * rendered, and link targets are limited to http(s), mailto and tel
 * (react-markdown's defaults), so a README cannot inject markup or
 * `javascript:` links.
 */
export function Markdown({children, className = ''}: { children: string; className?: string }) {
    return (
        <div className={`text-sm leading-relaxed text-fg wrap-anywhere ${className}`}>
            <ReactMarkdown remarkPlugins={[remarkGfm]} components={components}>
                {children}
            </ReactMarkdown>
        </div>
    )
}
