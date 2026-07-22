#!/usr/bin/env node
// Deterministically scrub real ER-C identities out of captured API responses
// (raw/*.json, from capture.sh) while preserving shape: member-count spread,
// name-length range, the numeric-UID-as-owner edge case, path-with-spaces edge
// case. Every real username/UID/project-or-folder-name is replaced by a fake
// of the same character class; the mapping is fixed (same input -> same
// output) so cross-references between projects.json / project-details.json /
// users.json stay consistent. See README.md for the full workflow and why this
// is NOT a fixed cipher like ROT13.
import {readFileSync, writeFileSync, mkdirSync, existsSync, readdirSync} from 'node:fs'
import {fileURLToPath} from 'node:url'

const HERE = new URL('./', import.meta.url)
const RAW = new URL('./raw/', HERE)
const OUT = new URL('./scrubbed/', HERE)
mkdirSync(OUT, {recursive: true})

const readJson = name => JSON.parse(readFileSync(new URL(name, RAW), 'utf-8'))
const writeJson = (name, data) =>
    writeFileSync(new URL(name, OUT), JSON.stringify(data, null, 2) + '\n')

// --- identity scrub ---------------------------------------------------------

const identityMap = new Map()
let userSeq = 0
let numSeq = 90000001

/** Same real string always maps to the same fake one. Numeric UIDs (the
 * unresolved-owner edge case, e.g. "4564654") map to a different fake numeric
 * string — a fresh sequential number, never truncated, so two different real
 * IDs can't collide onto the same fake one; everything else maps to "userN"
 * in order of first appearance. */
function scrubIdentity(real) {
    if (identityMap.has(real)) return identityMap.get(real)
    const isNumeric = /^\d+$/.test(real)
    const fake = isNumeric ? String(numSeq++) : `user${++userSeq}`
    identityMap.set(real, fake)
    return fake
}

// --- project/folder name scrub ---------------------------------------------
// Preserve length, case pattern, and punctuation/digit positions, but replace
// letters/digits with a *keyed pseudo-random* substitution seeded from the
// real string itself — not a fixed cipher like ROT13, which decodes with one
// click on any public tool. Without this script's per-string seed there is no
// known way back from the fake string to the real one.

function fnv1a(str) {
    let h = 0x811c9dc5
    for (let i = 0; i < str.length; i++) {
        h ^= str.charCodeAt(i)
        h = Math.imul(h, 0x01000193)
    }
    return h >>> 0
}

/** xorshift32, reseeded per character position so adjacent letters don't repeat. */
function pseudoRandomAt(seed, position) {
    let x = (seed ^ Math.imul(position + 1, 0x9e3779b1)) >>> 0
    x ^= x << 13; x >>>= 0
    x ^= x >>> 17
    x ^= x << 5; x >>>= 0
    return x
}

const nameMap = new Map()
function scrubName(real) {
    if (nameMap.has(real)) return nameMap.get(real)
    const seed = fnv1a(real)
    let out = ''
    for (let i = 0; i < real.length; i++) {
        const ch = real[i]
        const r = pseudoRandomAt(seed, i)
        if (/[a-z]/.test(ch)) out += String.fromCharCode(97 + (r % 26))
        else if (/[A-Z]/.test(ch)) out += String.fromCharCode(65 + (r % 26))
        else if (/[0-9]/.test(ch)) out += String(r % 10)
        else out += ch // punctuation/space/underscore/hyphen kept as-is (that's the shape)
    }
    nameMap.set(real, out)
    return out
}

/** Scrub every `/`-delimited segment of a path through the same nameMap, so a
 * path string (relpath, browse `path`, breadcrumb `full`) never carries a real
 * folder name — including shared parent segments like "adhoc" that a
 * last-segment-only replace would otherwise leave exposed. `er-c-data` is the
 * app's public root label, not an identity, and is left as-is. */
function scrubPath(path) {
    return path
        .split('/')
        .map(seg => (seg === '' || seg === 'er-c-data' ? seg : scrubName(seg)))
        .join('/')
}

function scrubProject(p, scrubbedName) {
    return {
        ...p,
        name: scrubbedName,
        owner: scrubIdentity(p.owner),
        members: p.members.map(scrubIdentity),
        relpath: scrubPath(p.relpath),
    }
}

// --- selection ---------------------------------------------------------------
// Which of the (likely hundreds of) real projects to include in the fixture
// subset. Edit selected-projects.json for a fresh capture — pick for variety
// (member-count spread, name-length range), not for volume; the fixtures exist
// to test shapes, not to mirror production at scale.

const SELECTED_PROJECTS = JSON.parse(
    readFileSync(new URL('./selected-projects.json', HERE), 'utf-8'),
)

// projects.json
if (existsSync(new URL('projects.json', RAW))) {
    const projectsRaw = readJson('projects.json')
    const selected = projectsRaw.projects.filter(p => SELECTED_PROJECTS.includes(p.name))
    const missing = SELECTED_PROJECTS.filter(n => !selected.some(p => p.name === n))
    if (missing.length) console.warn(`selected-projects.json names not found in raw capture: ${missing.join(', ')}`)
    writeJson('projects.json', {
        errno: 0,
        projects: selected.map(p => scrubProject(p, scrubName(p.name))),
    })
}

// project-details.json (the one fully-captured project)
if (existsSync(new URL('project-details.json', RAW))) {
    const detailsRaw = readJson('project-details.json')
    writeJson('project-details.json', {
        errno: 0,
        project: scrubProject(detailsRaw.project, scrubName(detailsRaw.project.name)),
        datasets: detailsRaw.datasets, // session/dataset names are demo/test data, not identities
    })
}

// users.json — identities already touched above, plus filler to reach a
// realistic-sized picker list without pulling more real names.
if (existsSync(new URL('users.json', RAW))) {
    const knownFake = [...identityMap.values()].filter(v => v.startsWith('user'))
    const filler = Array.from({length: 8}, (_, i) => `user${userSeq + i + 1}`)
    writeJson('users.json', {errno: 0, users: [...new Set([...knownFake, ...filler])].sort()})
}

// browse-*.json — every captured directory listing.
function scrubAccess(access) {
    const out = {}
    for (const [k, v] of Object.entries(access)) out[k] = v ? `https://example.invalid/${k}` : v
    return out
}
const rawDir = fileURLToPath(RAW)
const browseFiles = existsSync(rawDir)
    ? readdirSync(rawDir).filter(f => /^browse-.*\.json$/.test(f))
    : []
for (const file of browseFiles) {
    const browseRaw = readJson(file)
    const scrubbedPyramid = browseRaw.path_pyramid.map(
        ([full, tail]) => [full, tail === 'er-c-data' ? tail : scrubName(tail)],
    )
    const scrubbedMembers = browseRaw.members.map(m => {
        if (m.name === '..') return {...m, access: scrubAccess(m.access)}
        return {...m, name: scrubName(m.name), access: scrubAccess(m.access)}
    })
    writeJson(file, {
        errno: 0,
        path: scrubPath(browseRaw.path),
        info: {
            ...browseRaw.info,
            name: scrubName(browseRaw.info.name),
            project: browseRaw.info.project && scrubName(browseRaw.info.project),
        },
        access: scrubAccess(browseRaw.access),
        path_pyramid: scrubbedPyramid.map(([full, tail]) => [scrubPath(full), tail]),
        members: scrubbedMembers.map(m => ({...m, path: scrubPath(m.path)})),
    })
}

// share-created.json / shares-with-active.json (from capture-share.sh) — a
// share's credentials are already throwaway (created and deleted by that
// script), but dest_path still embeds real project/dataset names.
if (existsSync(new URL('share-created.json', RAW))) {
    const share = readJson('share-created.json')
    const parts = share.dest_path.split('/').filter(Boolean)
    const scrubbedParts = parts.map((seg, i) => (i < parts.length - 2 ? scrubName(seg) : seg))
    writeJson('share-created.json', {...share, dest_path: '/' + scrubbedParts.join('/')})
}

// --- report ------------------------------------------------------------

console.log(`${identityMap.size} identities scrubbed, ${nameMap.size} names/paths scrubbed`)
console.log('\nidentity map:')
for (const [real, fake] of identityMap) console.log(`  ${real} -> ${fake}`)
console.log('\nname map:')
for (const [real, fake] of nameMap) console.log(`  ${real} -> ${fake}`)
