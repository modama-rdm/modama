const dateFormatter = new Intl.DateTimeFormat('en-US', {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
})

/** Format a Unix epoch (seconds, as the API returns) like "Jun 16, 2025". */
export function formatDate(epochSeconds: number): string {
    return dateFormatter.format(new Date(epochSeconds * 1000))
}
