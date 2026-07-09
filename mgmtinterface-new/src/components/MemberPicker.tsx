import {useState} from 'react'
import {availableMembers} from '../lib/projectsUtil'
import {Avatar} from './ui/Avatar'
import {XIcon} from './ui/icons'

function MemberChip({name, onRemove}: { name: string; onRemove: () => void }) {
    return (
        <span
            className="inline-flex items-center gap-1.5 rounded-full bg-surface-muted py-0.5 pl-0.5 pr-1.5 text-sm text-fg">
            <Avatar username={name} size={20}/>
            {name}
            <button
                type="button"
                aria-label={`Remove ${name}`}
                onClick={onRemove}
                className="inline-flex h-4 w-4 items-center justify-center rounded-full text-fg-faint hover:bg-surface hover:text-fg"
            >
                <XIcon className="h-3 w-3"/>
            </button>
        </span>
    )
}

/**
 * Member multi-select used by New Project and Manage Members: chips for the
 * current selection + a typeahead listing `availableMembers` (every known user
 * minus the owner and those already chosen). Adds on `onMouseDown` so the click
 * lands before the input's blur closes the dropdown.
 */
export function MemberPicker({
                                 allUsers,
                                 owner,
                                 selected,
                                 onChange,
                                 helpText = 'You are the owner. Members can read and write files in the project.',
                                 optional = false,
                             }: {
    allUsers: string[]
    owner: string
    selected: string[]
    onChange: (next: string[]) => void
    helpText?: string
    /** Mark the field "(optional)" in the label, per the New Project design. */
    optional?: boolean
}) {
    const [query, setQuery] = useState('')
    const [open, setOpen] = useState(false)
    const candidates = availableMembers(allUsers, owner, selected, query)

    const add = (name: string) => {
        onChange([...selected, name])
        setQuery('')
    }
    const remove = (name: string) => onChange(selected.filter((m) => m !== name))

    return (
        <div>
            <label className="mb-1.5 block text-sm font-semibold text-fg-muted">
                Members{' '}
                {optional && (
                    <span className="font-normal text-fg-faint">(optional)</span>
                )}
            </label>
            <div className="relative">
                <div
                    className="flex flex-wrap items-center gap-1.5 rounded-lg border border-default bg-surface px-2 py-2 focus-within:border-primary focus-within:shadow-[0_0_0_3px_var(--ring)]">
                    {selected.map((m) => (
                        <MemberChip key={m} name={m} onRemove={() => remove(m)}/>
                    ))}
                    <input
                        value={query}
                        onChange={(e) => setQuery(e.target.value)}
                        onFocus={() => setOpen(true)}
                        onBlur={() => setOpen(false)}
                        placeholder={selected.length === 0 ? 'Add members…' : ''}
                        className="min-w-32 flex-1 bg-transparent px-1.5 py-1 text-sm text-fg placeholder:text-fg-faint focus:outline-none"
                    />
                </div>
                {open && candidates.length > 0 && (
                    <ul className="absolute z-20 mt-1 max-h-56 w-full overflow-auto rounded-lg border border-default bg-surface py-1 shadow-lg">
                        {candidates.map((u) => (
                            <li key={u}>
                                {/* onMouseDown fires before the input's blur, so the click registers. */}
                                <button
                                    type="button"
                                    onMouseDown={(e) => {
                                        e.preventDefault()
                                        add(u)
                                    }}
                                    className="flex w-full items-center gap-2 px-3 py-2 text-left text-sm text-fg hover:bg-surface-muted"
                                >
                                    <Avatar username={u} size={22}/>
                                    {u}
                                </button>
                            </li>
                        ))}
                    </ul>
                )}
            </div>
            <p className="mt-1.5 text-xs text-fg-faint">{helpText}</p>
        </div>
    )
}
