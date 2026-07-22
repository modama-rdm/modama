#!/usr/bin/env node
// Turn scrubbed/*.json (from scrub.mjs) into the typed fixture modules under
// ../../src/test/fixtures/. Review scrubbed/*.json yourself before running
// this — it's the last step before real (scrubbed) data becomes part of the
// committed test suite.
import {readFileSync, writeFileSync, existsSync} from 'node:fs'

const HERE = new URL('./', import.meta.url)
const SCRUBBED = new URL('./scrubbed/', HERE)
const FIXTURES = new URL('../../src/test/fixtures/', HERE)

const readJson = name => JSON.parse(readFileSync(new URL(name, SCRUBBED), 'utf-8'))
const write = (name, content) => writeFileSync(new URL(name, FIXTURES), content)

if (existsSync(new URL('projects.json', SCRUBBED))) {
    const {projects} = readJson('projects.json')
    write('projects.ts', `import type {ListProjectsResponse} from '../../api/types'

/**
 * A subset of the real production project list, scrubbed (see README.md in
 * this directory). Regenerate via scripts/fixture-capture/ rather than editing
 * by hand — see that directory's README for the capture -> scrub -> write
 * workflow.
 */
export const FIXTURE_PROJECTS: ListProjectsResponse['projects'] = ${JSON.stringify(projects, null, 4)}
`)
    console.log('wrote src/test/fixtures/projects.ts')
}

if (existsSync(new URL('project-details.json', SCRUBBED))) {
    const {project, datasets} = readJson('project-details.json')
    write('project.ts', `import type {ProjectDetailsResponse} from '../../api/types'

/**
 * A real project's full details, scrubbed (see README.md). Session/dataset
 * names are kept verbatim — demo/test data, not identities. Regenerate via
 * scripts/fixture-capture/ rather than editing by hand.
 */
export const FIXTURE_PROJECT: ProjectDetailsResponse['project'] = ${JSON.stringify(project, null, 4)}

export const FIXTURE_DATASETS: ProjectDetailsResponse['datasets'] = ${JSON.stringify(datasets, null, 4)}
`)
    console.log('wrote src/test/fixtures/project.ts')
}

if (existsSync(new URL('users.json', SCRUBBED))) {
    const {users} = readJson('users.json')
    write('users.ts', `/**
 * A subset of the real production user directory, scrubbed (see README.md).
 * Regenerate via scripts/fixture-capture/ rather than editing by hand.
 */
export const FIXTURE_USERS: string[] = ${JSON.stringify(users, null, 4)}
`)
    console.log('wrote src/test/fixtures/users.ts')
}

// browse-1.json -> browse.ts by convention; additional browse-N.json captures
// are left in scrubbed/ for manual use (no fixed convention for more than one).
if (existsSync(new URL('browse-1.json', SCRUBBED))) {
    const body = readJson('browse-1.json')
    delete body.errno
    write('browse.ts', `import type {BrowseSuccess} from '../../api/types'

/**
 * A real production directory listing, scrubbed (see README.md). Regenerate
 * via scripts/fixture-capture/ rather than editing by hand.
 */
export const FIXTURE_BROWSE: BrowseSuccess = ${JSON.stringify({errno: 0, ...body}, null, 4)}
`)
    console.log('wrote src/test/fixtures/browse.ts')
}

console.log('\nNOTE: share.ts is not generated here — see capture-share.sh and README.md.')
