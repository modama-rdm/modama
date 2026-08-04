from urllib.parse import urlsplit

from fastapi import Request, Response

from .config import Settings
from .errors import APIError
from .permissions import user_exists
from .session import (
    SessionCodec,
    SessionData,
    SessionInvalid,
    clear_session_cookie,
    set_session_cookie,
)
from .throttle import FailureThrottle

_SAFE_METHODS = frozenset({"GET", "HEAD", "OPTIONS"})


def settings_of(request: Request) -> Settings:
    return request.app.state.settings


def codec_of(request: Request) -> SessionCodec:
    return request.app.state.session_codec


def throttle_of(request: Request) -> FailureThrottle:
    return request.app.state.login_throttle


def enforce_same_origin(request: Request) -> None:
    """
    CSRF guard for state-changing requests.

    Cookies are attached by the browser automatically, which the previous Basic
    scheme was immune to (JS had to add the header). `SameSite=Lax` already
    blocks cross-site POSTs in current browsers; this is the second line, in the
    order OWASP recommends: Origin first, Referer as fallback.

    A request with *neither* header is allowed: browsers have sent Origin on
    cross-site state-changing requests for years, so absence means a non-browser
    caller (curl, a script, the test suite) rather than the attack this defends
    against.
    """
    if request.method in _SAFE_METHODS:
        return

    allowed = {_expected_origin(request)} | settings_of(request).extra_origins

    origin = request.headers.get("origin")
    if origin is not None:
        if origin not in allowed:
            raise APIError(403, "bad_origin", "Cross-origin request rejected.")
        return

    referer = request.headers.get("referer")
    if referer is not None and _origin_of(referer) not in allowed:
        raise APIError(403, "bad_origin", "Cross-origin request rejected.")


def _origin_of(url: str) -> str | None:
    parts = urlsplit(url)
    if not parts.scheme or not parts.netloc:
        return None
    return f"{parts.scheme}://{parts.netloc}"


def _expected_origin(request: Request) -> str:
    # nginx sets both headers; falling back to the request URL keeps this
    # working when the service is hit directly (local dev, container probes).
    scheme = request.headers.get("x-forwarded-proto", request.url.scheme)
    host = request.headers.get("host", request.url.netloc)
    return f"{scheme}://{host}"


# Declared `def`, not `async def`, on purpose: the refresh path makes a blocking
# HTTP call to the permissions service, and FastAPI runs sync dependencies in a
# threadpool instead of on the event loop.
def current_session(request: Request, response: Response) -> SessionData:
    """
    The signed-in user, refreshing the cookie when it has gone stale.

    Refreshing on an interval rather than on every request is what keeps the
    permissions service out of the hot path — otherwise this would re-create the
    old Basic behaviour of one upstream call per request.
    """
    settings = settings_of(request)
    token = request.cookies.get(settings.effective_cookie_name)
    if not token:
        raise APIError(401, "not_authenticated", "Sign in to continue.")

    try:
        data, age_seconds = codec_of(request).read(token)
    except SessionInvalid as exc:
        clear_session_cookie(response, settings)
        raise APIError(401, "session_expired", "Your session has expired.") from exc

    if age_seconds >= settings.session_refresh_seconds:
        if not user_exists(settings, data["username"]):
            clear_session_cookie(response, settings)
            raise APIError(401, "session_expired", "Your session has expired.")
        set_session_cookie(
            response,
            settings,
            codec_of(request).issue(data["username"], issued=data["issued"]),
        )

    return data
