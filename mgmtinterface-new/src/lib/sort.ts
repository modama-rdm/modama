/**
 * Column sorting shared by the tables with clickable headers (#75): what a
 * header click does, and reading a remembered sort back from storage. What each
 * column compares lives with its table (`sortProjects`, `sortSessions`).
 */

export type SortDir = 'asc' | 'desc'

export interface Sort<K extends string> {
    key: K
    dir: SortDir
}

/** A table's sortable columns, each with the direction a first click sorts it, and its starting sort. */
export interface SortSpec<K extends string> {
    firstDir: Record<K, SortDir>
    initial: Sort<K>
}

// Case-insensitive, and digit runs compare as numbers: "run2" before "run10".
export const compareNames = new Intl.Collator(undefined, {numeric: true}).compare

/**
 * The sort after a click on a column header: the active column flips
 * direction, any other starts the way people expect it to (names A - Z, dates
 * newest first, as its `firstDir` says).
 */
export function nextSort<K extends string>(spec: SortSpec<K>, current: Sort<K>, key: K): Sort<K> {
    if (current.key === key) return {key, dir: current.dir === 'asc' ? 'desc' : 'asc'}
    return {key, dir: spec.firstDir[key]}
}

/**
 * Read back a stored sort. Anything that isn't one of this table's (nothing
 * stored, a hand-edited value, a column a later version dropped) gives its
 * starting sort.
 */
export function parseSort<K extends string>(spec: SortSpec<K>, raw: string | null): Sort<K> {
    try {
        const {key, dir} = JSON.parse(raw ?? '') as Record<string, unknown>
        const columns: string[] = Object.keys(spec.firstDir)
        if (typeof key === 'string' && columns.includes(key) && (dir === 'asc' || dir === 'desc')) {
            return {key: key as K, dir}
        }
    } catch {
        /* not JSON, or JSON null - fall through to the starting sort */
    }
    return spec.initial
}
