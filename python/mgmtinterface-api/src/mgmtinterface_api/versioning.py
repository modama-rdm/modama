"""
API version negotiation: a required request header, and no fallback.

`modama-agent` — and anything else built against this API — is installed locally
on lab machines, and updating those installations is slow and not on our
schedule. So this server has to serve several contract versions at once, and,
the part that actually matters, it must never *guess* which one a caller wants.
A client that does not say gets an error rather than the newest version:
silently upgrading a two-year-old lab client on our next deploy is precisely the
failure this exists to prevent.

The version is an integer in the ``X-Modama-API-Version`` request header,
matching the integer protocol versions the agent's discovery message already
uses. It is deliberately not in the URL: a client pins its version once, in one
config value, instead of rewriting every path it builds, and this service's
route table stays identical across versions. The cost is that the version no
longer shows up in an nginx access log for free, which is why the resolved
version is echoed on the response and logged per client below.

The rules, all of them chosen rather than inherited:

* No header -> 400. This API has no client that predates the rule, so requiring
  an explicit pin is free now and impossible later.
* Unknown version -> 400, never 404, so the reason is readable in a lab log.
* The resolved version is echoed back, and ``Vary`` names the header, so no
  cache can hand one version's body to a client that asked for another.
* Deprecation and sunset dates are advertised per version (RFC 9745, RFC 8594)
  and listed by ``GET /versions``, so a client can warn its own operator well
  before anything is switched off.

Retiring a version is an *observation*, not a calendar promise: the "first
request" lines logged here tell you which clients are still asking for what, and
a version leaves `SUPPORTED_VERSIONS` only once nothing has asked for it in a
long while.
"""

import logging
from collections.abc import Awaitable, Callable
from dataclasses import dataclass
from datetime import datetime
from email.utils import format_datetime

from fastapi import Request, Response

from .config import API_PREFIX
from .errors import APIError

log = logging.getLogger(__name__)

#: Where a caller states which contract it was built against.
API_VERSION_HEADER = "X-Modama-API-Version"
#: Sent on every response, so a client that guessed wrong learns what exists
#: without having to find the documentation first.
SUPPORTED_HEADER = "X-Modama-API-Versions-Supported"


@dataclass(frozen=True)
class Deprecation:
    """When a version was announced as going away, and when it stops working."""

    since: datetime
    sunset: datetime


#: Every contract version this server still answers, oldest first.
SUPPORTED_VERSIONS: tuple[int, ...] = (1,)

LATEST_VERSION: int = max(SUPPORTED_VERSIONS)

#: Versions that still work but are on their way out. The shape, once there is
#: one to add:
#:
#:     1: Deprecation(
#:         since=datetime(2027, 1, 4, tzinfo=timezone.utc),
#:         sunset=datetime(2028, 1, 4, tzinfo=timezone.utc),
#:     )
DEPRECATIONS: dict[int, Deprecation] = {}

#: Paths that answer without a version: the container's liveness probe, and the
#: discovery endpoint a client calls precisely *because* it does not yet know
#: which versions exist. The docs are listed for the reader's benefit — FastAPI
#: registers those as plain Starlette routes, which app-wide dependencies never
#: reach anyway.
UNVERSIONED_PATHS = frozenset(
    {
        f"{API_PREFIX}/healthz",
        f"{API_PREFIX}/versions",
        f"{API_PREFIX}/docs",
        f"{API_PREFIX}/openapi.json",
    }
)

# Bound on the per-boot "first request" log: a runaway User-Agent must not turn
# into an unbounded set. In practice there are a handful of distinct clients.
_MAX_TRACKED_CLIENTS = 200


def supported_versions_header() -> str:
    return ", ".join(str(version) for version in SUPPORTED_VERSIONS)


def require_api_version(request: Request) -> None:
    """
    Resolve the caller's contract version, or refuse the request.

    Registered app-wide rather than per-router, for the same reason the CSRF
    check is: a router added later cannot forget it.
    """
    if request.url.path.rstrip("/") in UNVERSIONED_PATHS:
        return

    raw = request.headers.get(API_VERSION_HEADER)
    if raw is None:
        log.warning(
            "Request to %s with no %s header (client: %s)",
            request.url.path,
            API_VERSION_HEADER,
            request.headers.get("user-agent", "unknown"),
        )
        raise APIError(
            400,
            "missing_api_version",
            f"This API requires the {API_VERSION_HEADER} header. "
            f"Supported versions: {supported_versions_header()}.",
        )

    version: int | None
    try:
        version = int(raw.strip())
    except ValueError:
        # A bare integer only — no "v1". One accepted spelling means a client
        # cannot half-work by sending a form we happen to tolerate today.
        version = None

    if version is None or version not in SUPPORTED_VERSIONS:
        log.warning(
            "Request to %s for unsupported API version %r (client: %s)",
            request.url.path,
            raw,
            request.headers.get("user-agent", "unknown"),
        )
        raise APIError(
            400,
            "unsupported_api_version",
            f"API version {raw!r} is not supported by this server. "
            f"Supported versions: {supported_versions_header()}.",
        )

    request.state.api_version = version
    _note_client(request, version)


def api_version(request: Request) -> int:
    """
    The resolved version, for a handler whose behaviour differs between them.

    Always set by the time a handler runs: `require_api_version` is app-wide and
    raises rather than falling through.
    """
    return int(request.state.api_version)


def _note_client(request: Request, version: int) -> None:
    """
    Log the first request from each (version, client) pair since this boot.

    This is what makes retiring a version safe: "is any lab still on version 1"
    has to be answerable from the logs *before* anything is switched off. One
    line per pair rather than one per request — modama-agent already identifies
    itself as ``modama-agent/<git-version>``, so the pair is meaningful.
    """
    seen: set[tuple[int, str]] = request.app.state.api_version_clients
    client = (request.headers.get("user-agent") or "unknown")[:120]
    key = (version, client)
    if key in seen or len(seen) >= _MAX_TRACKED_CLIENTS:
        return
    seen.add(key)
    log.info("First request this boot for API version %d from %s", version, client)


async def add_version_headers(
    request: Request, call_next: Callable[[Request], Awaitable[Response]]
) -> Response:
    """
    Advertise the version contract on every response, errors included.

    Middleware rather than a handler concern so that responses produced by
    exception handlers — the two 400s above among them — carry these headers
    too: a client that got the version wrong is exactly the one that needs to be
    told what is on offer.
    """
    response = await call_next(request)

    response.headers[SUPPORTED_HEADER] = supported_versions_header()

    version = getattr(request.state, "api_version", None)
    if version is None:
        return response

    response.headers[API_VERSION_HEADER] = str(version)
    # Same URL, different bodies per version: without this a shared cache would
    # happily serve one client's version to another.
    _append_vary(response, API_VERSION_HEADER)

    deprecation = DEPRECATIONS.get(version)
    if deprecation is not None:
        # RFC 9745: a structured-field date — seconds since the epoch after '@'.
        response.headers["Deprecation"] = f"@{int(deprecation.since.timestamp())}"
        # RFC 8594: a plain HTTP-date.
        response.headers["Sunset"] = format_datetime(deprecation.sunset, usegmt=True)

    return response


def _append_vary(response: Response, field: str) -> None:
    existing = response.headers.get("vary")
    if not existing:
        response.headers["vary"] = field
        return
    if field.lower() in {part.strip().lower() for part in existing.split(",")}:
        return
    response.headers["vary"] = f"{existing}, {field}"
