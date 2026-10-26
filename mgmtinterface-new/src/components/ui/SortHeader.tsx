import type {Sort} from '../../lib/sort'
import {ArrowDownIcon, ArrowUpIcon, ChevronsUpDownIcon} from './icons'

/**
 * A table header that sorts by its column (#75). `aria-sort` sits on the active
 * one only; the others show a faint two-way arrow so they read as clickable.
 * `className` is the table's own `<th>` styling.
 */
export function SortHeader<K extends string>({
                                                 label,
                                                 column,
                                                 sort,
                                                 onSort,
                                                 className,
                                             }: {
    label: string
    column: K
    sort: Sort<K>
    onSort: (key: K) => void
    className: string
}) {
    const active = sort.key === column
    const Icon = !active ? ChevronsUpDownIcon : sort.dir === 'asc' ? ArrowUpIcon : ArrowDownIcon
    return (
        <th
            className={className}
            aria-sort={active ? (sort.dir === 'asc' ? 'ascending' : 'descending') : undefined}
        >
            <button
                type="button"
                onClick={() => onSort(column)}
                className={`group -mx-1 inline-flex items-center gap-1 rounded px-1 uppercase transition-colors hover:text-fg-muted focus:outline-none focus:shadow-[0_0_0_3px_var(--ring)] ${active ? 'text-fg-muted' : ''}`}
            >
                {label}
                <Icon
                    className={`h-3 w-3 ${active ? '' : 'opacity-50 group-hover:opacity-100'}`}
                />
            </button>
        </th>
    )
}
