import {describe, expect, it} from 'vitest'
import {stripBasename} from './basePath'

describe('stripBasename', () => {
    it('drops the basename so the path can be re-mounted after login', () => {
        // The regression: /new/projects must become /projects, otherwise
        // navigate() re-adds /new and lands on /new/new/projects (404).
        expect(stripBasename('/new/projects', '/new')).toBe('/projects')
        expect(stripBasename('/new/projects/foo/members', '/new')).toBe(
            '/projects/foo/members',
        )
    })

    it('maps the bare basename to root', () => {
        expect(stripBasename('/new', '/new')).toBe('/')
    })

    it('is a no-op when the app is mounted at root (dev)', () => {
        expect(stripBasename('/projects', '/')).toBe('/projects')
    })

    it('does not strip a look-alike prefix that is not a path segment', () => {
        expect(stripBasename('/newsroom', '/new')).toBe('/newsroom')
    })
})
