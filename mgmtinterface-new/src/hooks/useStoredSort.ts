import {useState} from 'react'
import {nextSort, parseSort, type Sort, type SortSpec} from '../lib/sort'

/**
 * A table's sort column (#75), remembered in this browser under `storageKey` so
 * whoever looks things up by name doesn't click the header on every visit. A
 * preference like the theme (`useTheme`): without storage the click still
 * sorts, it just isn't kept.
 */
export function useStoredSort<K extends string>(
    storageKey: string,
    spec: SortSpec<K>,
): [Sort<K>, (key: K) => void] {
    const [sort, setSort] = useState<Sort<K>>(() => {
        try {
            return parseSort(spec, localStorage.getItem(storageKey))
        } catch {
            /* storage unavailable — start with the table's own order */
            return spec.initial
        }
    })

    function sortBy(key: K) {
        const next = nextSort(spec, sort, key)
        setSort(next)
        try {
            localStorage.setItem(storageKey, JSON.stringify(next))
        } catch {
            /* storage unavailable — sorted for this visit only */
        }
    }

    return [sort, sortBy]
}
