const dateFormatter = new Intl.DateTimeFormat('en-US', {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
})

const dateTimeFormatter = new Intl.DateTimeFormat('en-US', {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
})

/** Format a Unix epoch (seconds, as the API returns) like "Jun 16, 2025". */
export function formatDate(epochSeconds: number): string {
    return dateFormatter.format(new Date(epochSeconds * 1000))
}

/** Like {@link formatDate} but with time — "Jun 16, 2025, 02:30 PM". */
export function formatDateTime(epochSeconds: number): string {
    return dateTimeFormatter.format(new Date(epochSeconds * 1000))
}

/**
 * Default session name — today's local date with a trailing dash, e.g.
 * "2026-06-25-". Mirrors the legacy `default_dataset_name()`: people record data
 * per day, so we pre-fill the date and let them append a suffix.
 */
export function sessionNamePrefix(now: Date = new Date()): string {
    const year = now.getFullYear()
    const month = String(now.getMonth() + 1).padStart(2, '0')
    const day = String(now.getDate()).padStart(2, '0')
    return `${year}-${month}-${day}-`
}

/**
 * Human-readable byte size — "1.4 GB", "812 MB". Mirrors the legacy
 * `format_filesize` (binary units). `null` size is rendered by the caller.
 */
export function formatBytes(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`
    const units = ['KB', 'MB', 'GB', 'TB', 'PB']
    let value = bytes / 1024
    let unit = 0
    while (value >= 1024 && unit < units.length - 1) {
        value /= 1024
        unit++
    }
    return `${value.toFixed(value < 10 ? 1 : 0)} ${units[unit]}`
}

/** Markdown that may open a line: headings, quotes, list bullets and numbers. */
const LEADING_MARKDOWN = /^(?:#{1,6}(?:\s+|$)|>\s*|[-*+](?:\s+|$)|\d+[.)](?:\s+|$))+/
/** A line that is only a horizontal rule: `---`, `***`, `___`. */
const RULE = /^([-*_])(?:\s*\1){2,}$/

/**
 * What a list shows of a description: its first non-blank line, without
 * the Markdown that opens it, or null when there is nothing to show. The rest
 * is the caller's to cut short (CSS `truncate`) and to offer in full on hover.
 */
export function descriptionDigest(description: string | null | undefined): string | null {
    if (!description) return null
    for (const raw of description.split('\n')) {
        const line = raw.trim().replace(LEADING_MARKDOWN, '').trim()
        if (line && !RULE.test(line)) return line
    }
    return null
}
