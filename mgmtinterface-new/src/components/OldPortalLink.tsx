/**
 * The way back to the old portal, on the login page and in the footer of every
 * page: someone who is signed in should not have to log out to find it. The
 * old portal has the domain root while this UI is served under a sub-path, so
 * this is a plain link out of the app and not a route.
 */
export function OldPortalLink() {
    return (
        <a
            href="/"
            className="whitespace-nowrap rounded text-[13px] text-fg-subtle hover:text-primary hover:underline focus:outline-none focus:shadow-[0_0_0_3px_var(--ring)]"
        >
            Use the old portal
        </a>
    )
}
