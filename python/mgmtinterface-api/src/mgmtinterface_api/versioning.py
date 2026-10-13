"""
API version negotiation via a required request header.

`modama-agent` and other clients are installed on lab machines and updated on
the lab's schedule, so the server has to support several contract versions at
once. There is no default version: a request without a version header gets an
error instead of the newest version, so a deploy cannot silently move an old
client onto a new contract.

The version is an integer in the ``X-Modama-API-Version`` request header, in
line with the integer protocol versions in the agent's discovery message. It is
not part of the URL, so a client pins its version in one config value instead
of in every path it builds, and the route table is the same for all versions.
The downside is that the version does not appear in the nginx access log, which
is why the resolved version is echoed on the response and logged per client.

Rules:

* No header -> 400. There is no client older than this rule, so requiring the
  header costs nothing now and would be hard to introduce later.
* Unknown version -> 400 rather than 404, so the reason is visible in the
  client's log.
* The resolved version is echoed back and ``Vary`` lists the header, so a cache
  cannot serve one version's body to a client that asked for another.
* Deprecation and sunset dates are advertised per version (RFC 9745, RFC 8594)
  and listed by ``GET /versions``, so a client can warn its operator before a
  version is switched off.

A version is removed from `SUPPORTED_VERSIONS` once the "first request" log
lines show that no client has used it for a long time.
"""

import logging
import re
from collections.abc import Awaitable, Callable
from dataclasses import dataclass
from datetime import datetime
from email.utils import format_datetime
from typing import Annotated, Any

from fastapi import Header, Request, Response

from .config import API_PREFIX
from .errors import APIError, error_responses

log = logging.getLogger(__name__)

#: Request header carrying the contract version the client was built against.
API_VERSION_HEADER = "X-Modama-API-Version"
#: Response header listing the supported versions, sent on every response.
SUPPORTED_HEADER = "X-Modama-API-Versions-Supported"


@dataclass(frozen=True)
class Deprecation:
    """When a version was deprecated, and when it stops being served."""

    since: datetime
    sunset: datetime


#: Contract versions this server supports, oldest first.
SUPPORTED_VERSIONS: tuple[int, ...] = (1,)

LATEST_VERSION: int = max(SUPPORTED_VERSIONS)

#: Deprecated versions that are still served. Example entry:
#:
#:     1: Deprecation(
#:         since=datetime(2027, 1, 4, tzinfo=timezone.utc),
#:         sunset=datetime(2028, 1, 4, tzinfo=timezone.utc),
#:     )
DEPRECATIONS: dict[int, Deprecation] = {}

#: Paths that do not require a version header: the liveness probe and the
#: discovery endpoint, which a client calls before it knows which versions
#: exist. The docs routes are listed for completeness; FastAPI registers them
#: as plain Starlette routes, which app-wide dependencies do not reach anyway.
UNVERSIONED_PATHS = frozenset(
    {
        f"{API_PREFIX}/healthz",
        f"{API_PREFIX}/versions",
        f"{API_PREFIX}/docs",
        f"{API_PREFIX}/openapi.json",
    }
)

# Upper bound for the set of (version, client) pairs logged per boot, so that
# varying User-Agent strings cannot grow it without limit. There are only a
# handful of distinct clients in practice.
_MAX_TRACKED_CLIENTS = 200

#: The only accepted form of a version: a non-negative integer without sign or
#: leading zeros. Anything else is rejected as an unsupported version.
_VERSION_SPELLING = re.compile(r"0|[1-9][0-9]*")

#: HTTP methods an OpenAPI path item can contain. Other keys under a path
#: (`summary`, shared `parameters`) are not operations.
_OPENAPI_METHODS = frozenset(
    {"get", "put", "post", "delete", "options", "head", "patch", "trace"}
)


def supported_versions_header() -> str:
    return ", ".join(str(version) for version in SUPPORTED_VERSIONS)


#: OpenAPI documentation of the 400 raised by `require_api_version`. Attached
#: to the routers with versioned endpoints.
VERSION_ERROR_RESPONSES = error_responses(
    {
        400: (
            f"The `{API_VERSION_HEADER}` header is missing, or names a version "
            "this server does not support."
        )
    }
)

#: Declared as a typed header parameter rather than read from the raw request,
#: so that it appears in the OpenAPI document. Otherwise a generated client
#: would not send the header, and "Try it out" in the docs would always get a
#: 400. The parameter is optional in the signature on purpose: a missing header
#: must reach the check in `require_api_version`, which answers with this API's
#: error body, instead of producing FastAPI's 422 validation response. It is
#: typed `str` rather than `int` for the same reason: an unparseable value is
#: reported as a 400 by this layer.
ApiVersionHeader = Annotated[
    str | None,
    Header(
        alias=API_VERSION_HEADER,
        description=(
            "The contract version the client was built against, as a bare "
            "integer. Required on every endpoint except `/versions` and "
            "`/healthz`, which a client can call before it knows which "
            "version to use."
        ),
        examples=[str(LATEST_VERSION)],
    ),
]


def require_api_version(request: Request, raw: ApiVersionHeader = None) -> None:
    """
    Resolve the caller's contract version, or reject the request.

    Registered app-wide rather than per router, like the CSRF check, so that a
    router added later cannot omit it.
    """
    if request.url.path.rstrip("/") in UNVERSIONED_PATHS:
        return

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

    # Only a bare integer is accepted, no "v1". `int()` alone would also accept
    # "+1", "01" and "1_0", so the form is checked with a regex first. Accepting
    # a single form avoids clients relying on a variant that is tolerated today
    # and rejected later.
    spelling = _VERSION_SPELLING.fullmatch(raw.strip())
    version = int(spelling.group()) if spelling else None

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
    The resolved version, for handlers whose behaviour differs between versions.

    It is always set by the time a handler runs, because `require_api_version`
    is applied app-wide and raises on failure.
    """
    return int(request.state.api_version)


def _note_client(request: Request, version: int) -> None:
    """
    Log the first request from each (version, client) pair since this boot.

    The log shows which clients still use which version, which has to be known
    before a version is removed. One line per pair instead of one per request;
    modama-agent identifies itself as ``modama-agent/<git-version>``, so the
    pair identifies the client.
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
    Add the version headers to every response, including error responses.

    This is middleware rather than a dependency so that responses built by
    exception handlers, including the two 400s above, carry the headers too. A
    client that sent a wrong version needs the list of supported ones.
    """
    response = await call_next(request)

    response.headers[SUPPORTED_HEADER] = supported_versions_header()

    version = getattr(request.state, "api_version", None)
    if version is None:
        return response

    response.headers[API_VERSION_HEADER] = str(version)
    # The same URL has different bodies per version. Without Vary a shared cache
    # could serve one version's body to a client that asked for another.
    _append_vary(response, API_VERSION_HEADER)

    deprecation = DEPRECATIONS.get(version)
    if deprecation is not None:
        # RFC 9745: a structured-field date, seconds since the epoch after '@'.
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


def document_version_header(schema: dict[str, Any]) -> dict[str, Any]:
    """
    Correct how the version header appears in the generated document.

    `require_api_version` declares the header optional in Python on purpose: a
    missing header has to reach its own check, which answers with this API's
    ``{code, message}`` 400 instead of FastAPI's 422 validation response. The
    document must not repeat that, because a generated client reading
    ``required: false`` may omit the header, which is what declaring the
    parameter was meant to prevent.

    So the requirement is set here, after generation: the parameter is marked
    required on the routes that enforce it and removed from the routes that
    ignore it. The function is idempotent, because FastAPI caches the document
    and returns the same object on later calls.
    """
    for path, path_item in schema.get("paths", {}).items():
        exempt = path.rstrip("/") in UNVERSIONED_PATHS
        for method, operation in path_item.items():
            if method.lower() not in _OPENAPI_METHODS:
                continue
            parameters = operation.get("parameters")
            if not parameters:
                continue
            kept: list[dict[str, Any]] = []
            for parameter in parameters:
                if parameter.get("name") != API_VERSION_HEADER:
                    kept.append(parameter)
                elif not exempt:
                    parameter["required"] = True
                    parameter["schema"] = _without_null(parameter.get("schema", {}))
                    kept.append(parameter)
            if kept:
                operation["parameters"] = kept
            else:
                del operation["parameters"]
    return schema


def _without_null(schema: dict[str, Any]) -> dict[str, Any]:
    """
    Drop the null branch that `str | None` generates; a required header is never
    null.

    The schema is left unchanged unless it has exactly one non-null branch, so a
    future change to the annotation leads to an imprecise document rather than
    a wrong one.
    """
    branches = schema.get("anyOf")
    if not branches:
        return schema
    concrete = [branch for branch in branches if branch.get("type") != "null"]
    if len(concrete) != 1:
        return schema
    return {
        **{key: value for key, value in schema.items() if key != "anyOf"},
        **concrete[0],
    }
