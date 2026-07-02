import {useEffect, useState} from 'react'
import {useNavigation} from 'react-router-dom'

/**
 * Thin top-of-viewport progress bar shown during route transitions. React
 * Router blocks navigation while a route's loader (or action) runs; without a
 * cue, the app looks frozen between the click and the next screen. This is the
 * app-wide floor - every blocking navigation gets feedback.
 *
 * The bar only appears after a short delay, so fast (sub-150ms) transitions stay
 * silent
 */
export function NavigationProgress() {
    const navigation = useNavigation()
    const active = navigation.state !== 'idle'
    const [visible, setVisible] = useState(false)

    useEffect(() => {
        if (!active) return
        const t = setTimeout(() => setVisible(true), 150)
        // Reset on the way out (transition finished) rather than in the body, so
        // the bar hides as soon as navigation goes idle.
        return () => {
            clearTimeout(t)
            setVisible(false)
        }
    }, [active])

    if (!visible) return null

    return (
        <div
            className="fixed inset-x-0 top-0 z-60 h-1 overflow-hidden bg-primary/20"
            role="progressbar"
            aria-label="Loading"
        >
            <div
                className="h-full w-1/3 bg-primary shadow-[0_0_8px_var(--primary)] animate-[progressIndeterminate_1.1s_ease-in-out_infinite]"/>
        </div>
    )
}
