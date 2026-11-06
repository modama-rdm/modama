import {act, render, screen} from '@testing-library/react'
import {afterEach, beforeEach, describe, expect, it, vi} from 'vitest'
import {ToastProvider} from './Toast'
import {useToast, type ToastApi} from './toast-context'

/** Mount the provider and hand back its `notify`, as a screen would get it. */
function renderProvider() {
    let api: ToastApi | null = null
    function Grab() {
        api = useToast()
        return null
    }
    const utils = render(<ToastProvider><Grab/></ToastProvider>)
    return {...utils, notify: (message: string) => act(() => api!.notify(message))}
}

describe('ToastProvider', () => {
    beforeEach(() => {
        vi.useFakeTimers()
    })

    afterEach(() => {
        vi.useRealTimers()
    })

    it('shows a toast and takes it away after 3.5 seconds', () => {
        const {notify} = renderProvider()

        notify('Description saved.')
        expect(screen.getByText('Description saved.')).toBeInTheDocument()

        act(() => vi.advanceTimersByTime(3499))
        expect(screen.getByText('Description saved.')).toBeInTheDocument()

        act(() => vi.advanceTimersByTime(1))
        expect(screen.queryByText('Description saved.')).not.toBeInTheDocument()
    })

    it('leaves no timer running once it is unmounted', () => {
        const {notify, unmount} = renderProvider()

        notify('Description saved.')
        notify('Session "run 1" created.')
        expect(vi.getTimerCount()).toBe(2)

        unmount()
        expect(vi.getTimerCount()).toBe(0)
    })
})
