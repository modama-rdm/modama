from typing import Annotated
from urllib.parse import urlsplit

from fastapi import Request, Response, Security
from fastapi.security import APIKeyCookie

from .config import HOST_PREFIXED_SESSION_COOKIE_NAME, Settings
from .errors import APIError, error_responses
from .permissions import user_exists
from .session import (
    SessionCodec,
    SessionData,
    SessionInvalid,
    clear_session_cookie_header,
    set_session_cookie,
)
from .throttle import FailureThrottle

_SAFE_METHODS = frozenset({"GET", "HEAD", "OPTIONS"})

#: The session cookie as an OpenAPI security scheme, so that endpoints that
#: need a session are marked in the document and a generated client knows it
#: has to carry the cookie.
#:
#: `auto_error=False` because the 401 is raised by `current_session` with this
#: API's error body, not by FastAPI's security helper. The name is the
#: `__Host-` prefixed one that a real deployment sends. A plain-http
#: development instance drops the prefix, which is why `current_session` reads
#: the cookie through `settings` rather than through this dependency.
session_cookie_scheme = APIKeyCookie(
    name=HOST_PREFIXED_SESSION_COOKIE_NAME,
    scheme_name="session",
    description=(
        "Signed session cookie, set by `POST /auth/login` and sent back "
        "automatically by the browser. Requests without a usable one get 401."
    ),
    auto_error=False,
)

#: OpenAPI documentation of the errors raised in this module. Two sets, because
#: they apply to different routes: every endpoint behind `current_session` can
#: return 401, but only state-changing requests reach the CSRF check.
AUTH_ERROR_RESPONSES = error_responses(
    {401: "No session, or the session has expired. Sign in again."}
)
CSRF_ERROR_RESPONSES = error_responses(
    {403: "Rejected: the request came from another origin."}
)


def settings_of(request: Request) -> Settings:
    return request.app.state.settings


def codec_of(request: Request) -> SessionCodec:
    return request.app.state.session_codec


def throttle_of(request: Request) -> FailureThrottle:
    return request.app.state.login_throttle


def enforce_same_origin(request: Request) -> None:
    """
    CSRF check for state-changing requests.

    The browser attaches cookies automatically, unlike the Basic auth header of
    the Flask API, which JS had to add. `SameSite=Lax` already blocks
    cross-site POSTs in current browsers; this check is the second layer, in
    the order OWASP recommends: Origin first, Referer as fallback.

    A request with neither header is allowed. Browsers send Origin on
    cross-site state-changing requests, so a request without it comes from a
    non-browser client (curl, a script, the test suite).
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


#: Default port per scheme. A browser omits it from `Origin`, so a Host header
#: that carries it has to be normalized before comparing.
_DEFAULT_PORTS = {"http": "80", "https": "443"}


def _expected_origin(request: Request) -> str:
    # nginx sets both headers; the fallback to the request URL covers direct
    # access (local dev, container probes). The Host header must include the
    # port the browser used (nginx: $http_host, not $host). The test deployment
    # runs on :5001, and an origin without that port never matches what the
    # browser sends.
    scheme = request.headers.get("x-forwarded-proto", request.url.scheme)
    host = request.headers.get("host", request.url.netloc)
    return f"{scheme}://{_without_default_port(scheme, host)}"


def _without_default_port(scheme: str, host: str) -> str:
    """
    ``example.org:443`` -> ``example.org`` for https, and likewise :80 for http.

    Only an explicit default port is dropped. A Host header that spells it out
    would otherwise never match the `Origin`, which the browser sends without
    the port. Any other port is kept.
    """
    prefix, sep, port = host.rpartition(":")
    if sep and port == _DEFAULT_PORTS.get(scheme):
        return prefix
    return host


# Declared `def`, not `async def`, on purpose: the refresh path makes a blocking
# HTTP call to the permissions service, and FastAPI runs sync dependencies in a
# threadpool instead of on the event loop.
def current_session(
    request: Request,
    response: Response,
    # Only for the OpenAPI document. The value is read from `request.cookies`
    # below, under the configured cookie name.
    _scheme: Annotated[str | None, Security(session_cookie_scheme)] = None,
) -> SessionData:
    """
    The signed-in user. Refreshes the cookie once it is older than the refresh
    interval.

    Refreshing on an interval rather than on every request keeps the
    permissions service out of the per-request path. Otherwise this would be
    the same as Basic auth in the Flask API, with one upstream call per request.
    """
    settings = settings_of(request)
    token = request.cookies.get(settings.effective_cookie_name)
    if not token:
        raise APIError(401, "not_authenticated", "Sign in to continue.")

    # The cookie deletion travels as a header on the error, not on `response`:
    # an exception handler builds its own response, so anything written to the
    # injected one is discarded. `APIError` carries `Retry-After` for the login
    # throttle for the same reason.
    try:
        data, age_seconds = codec_of(request).read(token)
    except SessionInvalid as exc:
        raise APIError(
            401,
            "session_expired",
            "Your session has expired.",
            headers=clear_session_cookie_header(settings),
        ) from exc

    if age_seconds >= settings.session_refresh_seconds:
        if not user_exists(settings, data["username"]):
            raise APIError(
                401,
                "session_expired",
                "Your session has expired.",
                headers=clear_session_cookie_header(settings),
            )
        # The re-issued cookie keeps the original `issued` and the kind of
        # session, so a persistent session stays persistent and neither kind
        # can outlive its absolute lifetime.
        set_session_cookie(
            response,
            settings,
            codec_of(request).issue(
                data["username"], issued=data["issued"], persistent=data["persistent"]
            ),
            persistent=data["persistent"],
        )

    return data
