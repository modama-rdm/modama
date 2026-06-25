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
