/**
 * The search behind the projects list and a project's sessions table (#113).
 *
 * The query is split into words; an item matches when every word occurs
 * somewhere in its fields, in any order and regardless of case. Dash,
 * underscore, dot and whitespace are one and the same separator, so
 * "demo holo" finds `demo-holography` and "stem map" finds
 * `2026-09-30 4D-STEM map 01`. Matches come best first: the ones whose name
 * starts with the query, then the rest, each in the order they came in.
 *
 * This is not fuzzy in the edit-distance sense: "holgraphy" does not find
 * `holography`. Names are short and the lists small (hundreds of projects,
 * tens of sessions each), and lenient separators cover what people type, so
 * typo tolerance, which would need a scoring algorithm, is left out on purpose.
 * The fields are a list so that a description (#112) is one more entry.
 */

/** Every run of separators, to collapse into one space. */
const SEPARATORS = /[\s\-_.]+/g

/**
 * The form a field and a word are compared in: lowercase, every run of dash,
 * underscore, dot or whitespace one space, nothing at the ends.
 */
export function normalize(text: string): string {
    return text.toLowerCase().replace(SEPARATORS, ' ').trim()
}

/** The query's words, normalized; none for a query that is blank or only separators. */
function words(query: string): string[] {
    return query.split(/\s+/).map(normalize).filter((word) => word !== '')
}

function every(words: string[], haystacks: string[]): boolean {
    return words.every((word) => haystacks.some((text) => text.includes(word)))
}

/**
 * Whether every word of `query` occurs in at least one of `fields`. A blank
 * query matches everything.
 */
export function matches(fields: string[], query: string): boolean {
    return every(words(query), fields.map(normalize))
}

/**
 * The items matching `query`, best first: those whose first field (the name)
 * starts with the whole query, then the other matches, each group in the
 * order `items` had. A blank query gives `items` back as they are.
 */
export function rankBy<T>(items: T[], query: string, fields: (item: T) => string[]): T[] {
    const terms = words(query)
    if (terms.length === 0) return items
    const whole = terms.join(' ')
    const byPrefix: T[] = []
    const byWords: T[] = []
    for (const item of items) {
        const haystacks = fields(item).map(normalize)
        if (!every(terms, haystacks)) continue
        if (haystacks.length > 0 && haystacks[0].startsWith(whole)) byPrefix.push(item)
        else byWords.push(item)
    }
    return [...byPrefix, ...byWords]
}
