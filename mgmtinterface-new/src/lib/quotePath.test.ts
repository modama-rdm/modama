import {describe, expect, it} from 'vitest'
import {browsePath, browseUrl, quotePath} from './quotePath'

// Cases mirror python/mgmtinterface/tests/test_url_encoding.py so the two
// implementations can't drift apart silently.
describe('quotePath', () => {
    it.each([
        // simple test case for single quote:
        ["/stuff'", "/stuff'"],
        // all the "safe" chars are kept:
        ["/stuff/!$'()*,/:;?@", "/stuff/!$'()*,/:;?@"],
        // importantly, # is percent-encoded:
        ['/stuff/#hash', '/stuff/%23hash'],
        // we use plus-encoding:
        ['/spaces are double plus good/', '/spaces+are+double+plus+good/'],
        // non-ASCII is percent-encoded per UTF-8 byte (backend: add_path_to_query test):
        ["/data/prime'ä", "/data/prime'%C3%A4"],
        // backslash is a potential trouble maker:
        ['/weird/\\/', '/weird/%5C/'],
        // double quotes are encoded:
        ['""', '%22%22'],
        // & = + would corrupt the query string if left literal:
        ['/a&b=c+d', '/a%26b%3Dc%2Bd'],
    ])('encodes %s like the backend quote_path', (input, expected) => {
        expect(quotePath(input)).toBe(expected)
    })

    it.each([
        '/adhoc/sortpatterns',
        '/spaces are double plus good/',
        "/data/prime'ä",
        '/stuff/#hash',
        '/a&b=c+d',
    ])('round-trips %s through URLSearchParams (the loader read side)', (path) => {
        expect(new URLSearchParams(`path=${quotePath(path)}`).get('path')).toBe(path)
    })
})

describe('browseUrl', () => {
    // What the copy buttons on Browse and Project details hand out for pasting
    // into an ELN entry, so it must carry the quote_path encoding end to end.
    it('is absolute and keeps slashes literal', () => {
        expect(browseUrl('/adhoc/test-project')).toBe(
            `${window.location.origin}/browse?path=/adhoc/test-project`,
        )
    })

    it('encodes a path that would otherwise corrupt the query string', () => {
        expect(browseUrl("/adhoc/my stuff/prime'ä")).toBe(
            `${window.location.origin}/browse?path=/adhoc/my+stuff/prime'%C3%A4`,
        )
    })

    it('agrees with browsePath on the query part', () => {
        const path = '/adhoc/#hash&more'
        expect(browseUrl(path).endsWith(browsePath(path))).toBe(true)
    })
})
