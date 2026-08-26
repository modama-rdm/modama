"""
Tests for the generated OpenAPI document.

The document is built lazily, on the first request to `/docs` or
`/openapi.json` — so without these, a malformed `responses` mapping or a broken
description template would first surface in a browser, in a deployment, while
the suite stayed green. These assert the handful of things a client author
actually depends on, not the whole schema.
"""

from typing import Any

import pytest
from mgmtinterface_api import API_PREFIX, create_app
from mgmtinterface_api.config import Settings
from mgmtinterface_api.versioning import (
    API_VERSION_HEADER,
    LATEST_VERSION,
    SUPPORTED_VERSIONS,
)

LOGIN = f"{API_PREFIX}/auth/login"
SESSION = f"{API_PREFIX}/auth/session"
PROJECTS = f"{API_PREFIX}/projects"
VERSIONS = f"{API_PREFIX}/versions"
HEALTHZ = f"{API_PREFIX}/healthz"


@pytest.fixture
def schema(settings: Settings) -> dict[str, Any]:
    return create_app(settings).openapi()


def version_parameter(
    schema: dict[str, Any], path: str, method: str = "get"
) -> dict[str, Any] | None:
    parameters = schema["paths"][path][method].get("parameters", [])
    for parameter in parameters:
        if parameter["name"] == API_VERSION_HEADER:
            return parameter
    return None


def test_document_builds(schema: dict[str, Any]):
    """The whole point: a broken document must fail here, not in a browser."""
    assert schema["openapi"].startswith("3.")
    assert schema["info"]["version"] == str(LATEST_VERSION)


def test_landing_description_lists_the_supported_versions(schema: dict[str, Any]):
    # The description is a format template; an unfilled placeholder would leave
    # a literal brace in the rendered page.
    description = schema["info"]["description"]
    assert "{versions}" not in description
    assert str(SUPPORTED_VERSIONS[0]) in description


def test_every_documented_route_is_present(schema: dict[str, Any]):
    assert set(schema["paths"]) == {
        LOGIN,
        f"{API_PREFIX}/auth/logout",
        SESSION,
        PROJECTS,
        VERSIONS,
        HEALTHZ,
    }


def test_version_header_is_documented_as_required(schema: dict[str, Any]):
    """
    The reason the header is declared at all: a generated client has to send it.

    It is optional in the Python signature on purpose — a missing header must
    reach this API's own 400 rather than FastAPI's 422 — so the requirement is
    restated on the document afterwards. If that correction is ever dropped, a
    generated client silently stops sending the header and every call 400s.
    """
    for path, method in ((LOGIN, "post"), (SESSION, "get"), (PROJECTS, "get")):
        parameter = version_parameter(schema, path, method)
        assert parameter is not None, f"{method.upper()} {path} does not document it"
        assert parameter["required"] is True, f"{method.upper()} {path} says optional"
        # `str | None` generates an anyOf with a null branch; required means it
        # cannot be null, and a client generator should see a plain string.
        assert "anyOf" not in parameter["schema"]
        assert parameter["schema"]["type"] == "string"


@pytest.mark.parametrize("path", [VERSIONS, HEALTHZ])
def test_unversioned_routes_do_not_document_the_header(
    schema: dict[str, Any], path: str
):
    """These two ignore the header, so the document must not ask for it."""
    assert version_parameter(schema, path) is None


def test_session_cookie_is_declared_as_a_security_scheme(schema: dict[str, Any]):
    scheme = schema["components"]["securitySchemes"]["session"]
    assert scheme["type"] == "apiKey"
    assert scheme["in"] == "cookie"
    # The name as a real (Secure) deployment sends it.
    assert scheme["name"].startswith("__Host-")


def test_only_the_routes_needing_a_session_are_marked(schema: dict[str, Any]):
    assert schema["paths"][SESSION]["get"].get("security")
    assert schema["paths"][PROJECTS]["get"].get("security")
    # Signing in cannot require being signed in, and logging out of an expired
    # session answers 204 rather than 401.
    assert not schema["paths"][LOGIN]["post"].get("security")
    assert not schema["paths"][f"{API_PREFIX}/auth/logout"]["post"].get("security")
    assert not schema["paths"][VERSIONS]["get"].get("security")


def test_error_statuses_are_documented_with_the_house_body(schema: dict[str, Any]):
    """
    Each layer contributes the errors it can raise, so a route documents the
    union of its own and its dependencies'. Login is the richest case.
    """
    login = schema["paths"][LOGIN]["post"]["responses"]
    assert {"400", "401", "403", "429"} <= set(login)
    for status in ("400", "401", "403", "429"):
        ref = login[status]["content"]["application/json"]["schema"]["$ref"]
        assert ref.endswith("/ErrorResponse")

    # A route behind `current_session` gets the 401 but never the CSRF 403,
    # which only state-changing requests can reach.
    session = schema["paths"][SESSION]["get"]["responses"]
    assert "401" in session
    assert "403" not in session


def test_unversioned_routes_do_not_advertise_the_version_400(schema: dict[str, Any]):
    assert "400" not in schema["paths"][VERSIONS]["get"]["responses"]
    assert "400" not in schema["paths"][HEALTHZ]["get"]["responses"]


def test_error_model_documents_both_fields(schema: dict[str, Any]):
    properties = schema["components"]["schemas"]["ErrorResponse"]["properties"]
    assert set(properties) == {"code", "message"}
    assert properties["code"]["description"]
    assert properties["message"]["description"]


def test_document_is_stable_across_calls(settings: Settings):
    """
    FastAPI caches the document; the correction pass must be idempotent rather
    than compounding on a second read of `/openapi.json`.
    """
    app = create_app(settings)
    assert app.openapi() == app.openapi()
