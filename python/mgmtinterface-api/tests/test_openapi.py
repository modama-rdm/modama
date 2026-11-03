"""
Tests for the generated OpenAPI document.

The document is built lazily, on the first request to `/docs` or
`/openapi.json`. Without these tests, a malformed `responses` mapping or a
broken description template would only show up in a browser, in a deployment,
while the test suite stayed green. The tests cover the parts a client author
depends on, not the whole schema.
"""

from pathlib import Path
from typing import Any

import pytest
from mgmtinterface_api import API_PREFIX, create_app, openapi_snapshot
from mgmtinterface_api.config import Settings
from mgmtinterface_api.security import CSRF_ERROR_RESPONSES
from mgmtinterface_api.versioning import (
    API_VERSION_HEADER,
    LATEST_VERSION,
    SUPPORTED_VERSIONS,
)

LOGIN = f"{API_PREFIX}/auth/login"
SESSION = f"{API_PREFIX}/auth/session"
PROJECTS = f"{API_PREFIX}/projects"
PROJECT = f"{PROJECTS}/{{project}}"
PROJECT_USAGE = f"{PROJECT}/usage"
PROJECT_MEMBERS = f"{PROJECT}/members"
SESSIONS = f"{PROJECT}/sessions"
SEAL = f"{SESSIONS}/{{session}}/seal"
PROJECT_DESCRIPTION = f"{PROJECT}/description"
SESSION_DESCRIPTION = f"{SESSIONS}/{{session}}/description"
USERS = f"{API_PREFIX}/users"
INGEST_SHARES = f"{API_PREFIX}/ingest-shares"
INGEST_SHARE = f"{INGEST_SHARES}/{{share_id}}"
BROWSE = f"{API_PREFIX}/browse"
VERSIONS = f"{API_PREFIX}/versions"
HEALTHZ = f"{API_PREFIX}/healthz"

#: Every operation that needs a session, as (path, method). Used by the tests
#: that check what each such operation has to document.
SESSION_OPERATIONS = [
    (SESSION, "get"),
    (PROJECTS, "get"),
    (PROJECTS, "post"),
    (PROJECT, "get"),
    (PROJECT_USAGE, "get"),
    (PROJECT_MEMBERS, "put"),
    (SESSIONS, "post"),
    (SEAL, "post"),
    (PROJECT_DESCRIPTION, "put"),
    (SESSION_DESCRIPTION, "put"),
    (USERS, "get"),
    (INGEST_SHARES, "get"),
    (INGEST_SHARES, "post"),
    (INGEST_SHARE, "delete"),
    (BROWSE, "get"),
]


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
    """A broken document must fail here, not in a browser."""
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
        PROJECT,
        PROJECT_USAGE,
        PROJECT_MEMBERS,
        SESSIONS,
        SEAL,
        PROJECT_DESCRIPTION,
        SESSION_DESCRIPTION,
        USERS,
        INGEST_SHARES,
        INGEST_SHARE,
        BROWSE,
        VERSIONS,
        HEALTHZ,
    }


def test_version_header_is_documented_as_required(schema: dict[str, Any]):
    """
    The header is declared so that a generated client sends it.

    It is optional in the Python signature on purpose, because a missing header
    must reach this API's own 400 rather than FastAPI's 422, so the requirement
    is set on the document afterwards. If that correction is dropped, a
    generated client stops sending the header and every call gets a 400.
    """
    for path, method in ((LOGIN, "post"), *SESSION_OPERATIONS):
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
    for path, method in SESSION_OPERATIONS:
        operation = schema["paths"][path][method]
        assert operation.get("security"), f"{method.upper()} {path} is not marked"
        assert "401" in operation["responses"], f"{method.upper()} {path} lacks 401"
    # Signing in cannot require being signed in, and logging out of an expired
    # session answers 204 rather than 401.
    assert not schema["paths"][LOGIN]["post"].get("security")
    assert not schema["paths"][f"{API_PREFIX}/auth/logout"]["post"].get("security")
    assert not schema["paths"][VERSIONS]["get"].get("security")


def test_login_documents_the_persistent_flag(schema: dict[str, Any]):
    """
    The flag is how "keep me logged in" and a long-running client ask for a
    session without an idle timeout, so a generated client has to see it: a
    boolean, optional, off by default. Both session responses report the kind.
    """
    body = schema["paths"][LOGIN]["post"]["requestBody"]["content"]
    ref = body["application/json"]["schema"]["$ref"]
    request_model = schema["components"]["schemas"][ref.rsplit("/", 1)[1]]
    flag = request_model["properties"]["persistent"]
    assert flag["type"] == "boolean"
    assert flag["default"] is False
    assert "persistent" not in request_model.get("required", [])

    session_info = schema["components"]["schemas"]["SessionInfo"]
    assert session_info["properties"]["persistent"]["type"] == "boolean"
    assert "persistent" in session_info["required"]


def test_only_state_changing_operations_document_the_csrf_403(schema: dict[str, Any]):
    """
    The origin check applies app-wide but only to non-safe methods, so a GET
    must not advertise a 403 it can never answer with. A read may have a 403 of
    its own (browse: a folder the user may not read), but never the origin
    check's.
    """
    csrf = CSRF_ERROR_RESPONSES[403]["description"]
    for path, method in SESSION_OPERATIONS:
        responses = schema["paths"][path][method]["responses"]
        if method == "get":
            description = responses.get("403", {}).get("description")
            assert description != csrf, f"GET {path} documents the CSRF 403"
        else:
            assert "403" in responses, f"{method.upper()} {path} lacks the 403"


def test_project_details_embed_the_sessions(schema: dict[str, Any]):
    # Flat: the project's own fields plus `sessions`, named after the UI's
    # term, not the legacy `{project, datasets}` envelope.
    details = schema["components"]["schemas"]["ProjectDetails"]
    assert "sessions" in details["properties"]
    assert "name" in details["properties"]
    assert details["properties"]["sessions"]["items"]["$ref"].endswith("/Session")
    assert "usage_bytes" in details["properties"]
    assert set(schema["components"]["schemas"]["ProjectUsage"]["properties"]) == {
        "usage_bytes"
    }


def test_members_are_set_as_a_whole_list(schema: dict[str, Any]):
    operation = schema["paths"][PROJECT_MEMBERS]["put"]
    ref = operation["requestBody"]["content"]["application/json"]["schema"]["$ref"]
    assert ref.endswith("/SetMembersRequest")
    request = schema["components"]["schemas"]["SetMembersRequest"]
    assert request["required"] == ["members"]
    assert "400" in operation["responses"]
    assert "404" in operation["responses"]
    # The answer is the project, not a dedicated response type.
    ok = operation["responses"]["200"]["content"]["application/json"]["schema"]
    assert ok["$ref"].endswith("/Project")


def test_descriptions_are_optional_everywhere(schema: dict[str, Any]):
    """
    Descriptions are additive: a nullable `description` on the project and the
    session that a client may ignore, and an optional one on both create
    requests.
    """
    schemas = schema["components"]["schemas"]
    for name in ("Project", "Session", "CreateProjectRequest", "CreateSessionRequest"):
        model = schemas[name]
        assert "description" in model["properties"], name
        assert "description" not in model.get("required", []), name
    for name in ("CreateProjectRequest", "CreateSessionRequest"):
        branches = schemas[name]["properties"]["description"]["anyOf"]
        assert {"type": "string", "maxLength": 10_000} in branches, name


@pytest.mark.parametrize(
    ("path", "answer"),
    [(PROJECT_DESCRIPTION, "/Project"), (SESSION_DESCRIPTION, "/Session")],
)
def test_descriptions_are_set_with_a_put(
    schema: dict[str, Any], path: str, answer: str
):
    operation = schema["paths"][path]["put"]
    ref = operation["requestBody"]["content"]["application/json"]["schema"]["$ref"]
    assert ref.endswith("/SetDescriptionRequest")
    assert schema["components"]["schemas"]["SetDescriptionRequest"]["required"] == [
        "description"
    ]
    ok = operation["responses"]["200"]["content"]["application/json"]["schema"]
    assert ok["$ref"].endswith(answer)
    assert {"400", "401", "403", "404"} <= set(operation["responses"])


def test_seal_and_close_document_their_outcomes(schema: dict[str, Any]):
    seal = schema["paths"][SEAL]["post"]
    # Everything it needs is in the path; a body would be an extra thing for a
    # client to get wrong.
    assert "requestBody" not in seal
    assert seal["responses"]["200"]["content"]["application/json"]["schema"][
        "$ref"
    ].endswith("/Session")
    assert {"400", "403", "404"} <= set(seal["responses"])

    close = schema["paths"][INGEST_SHARE]["delete"]
    assert "204" in close["responses"]
    assert "content" not in close["responses"]["204"]
    assert {"403", "404", "502"} <= set(close["responses"])


def test_share_list_is_a_list_of_the_create_response_type(schema: dict[str, Any]):
    listed = schema["paths"][INGEST_SHARES]["get"]["responses"]["200"]
    ref = listed["content"]["application/json"]["schema"]["$ref"]
    assert ref.endswith("/IngestShareList")
    items = schema["components"]["schemas"]["IngestShareList"]["properties"]["shares"]
    assert items["items"]["$ref"].endswith("/IngestShare")
    created = schema["paths"][INGEST_SHARES]["post"]["responses"]["201"]
    assert created["content"]["application/json"]["schema"]["$ref"].endswith(
        "/IngestShare"
    )


def test_ingest_share_documents_its_optional_creator(schema: dict[str, Any]):
    share = schema["components"]["schemas"]["IngestShare"]
    assert "created_by" not in share["required"]
    created_by = share["properties"]["created_by"]
    assert {"type": "null"} in created_by["anyOf"]
    assert created_by["description"]


@pytest.mark.parametrize("path", [PROJECTS, SESSIONS, INGEST_SHARES])
def test_create_endpoints_document_both_outcomes(schema: dict[str, Any], path: str):
    """
    An idempotent create has two success answers, 201 and 200, and a client
    author has to see both to handle a retry correctly. Both carry the
    resource, not the error body.
    """
    responses = schema["paths"][path]["post"]["responses"]
    assert {"200", "201"} <= set(responses)
    for status in ("200", "201"):
        ref = responses[status]["content"]["application/json"]["schema"]["$ref"]
        assert not ref.endswith("/ErrorResponse"), f"{status} documents the error body"
    assert responses["200"]["content"] == responses["201"]["content"]
    # State-changing, so the CSRF 403 applies, and a session is required.
    assert {"401", "403"} <= set(responses)


def test_share_create_documents_the_agent_binding(schema: dict[str, Any]):
    """
    The agent's `X-Agent-Token` and `resource` are optional, so the web
    interface's requests stay valid, and documented, so a generated client can
    send them.
    """
    create = schema["paths"][INGEST_SHARES]["post"]
    header = next(p for p in create["parameters"] if p["name"] == "X-Agent-Token")
    assert header["in"] == "header"
    assert not header.get("required", False)
    assert header["description"]
    request = schema["components"]["schemas"]["CreateIngestShareRequest"]
    assert "resource" in request["properties"]
    assert "resource" not in request["required"]
    assert "400" in create["responses"]


def test_project_conflict_is_documented(schema: dict[str, Any]):
    responses = schema["paths"][PROJECTS]["post"]["responses"]
    assert "409" in responses
    assert "409" not in schema["paths"][SESSIONS]["post"]["responses"]


def test_error_statuses_are_documented_with_the_house_body(schema: dict[str, Any]):
    """
    Each layer contributes the errors it can raise, so a route documents the
    union of its own and its dependencies' errors. Login has the most.
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
    FastAPI caches the document, so the correction pass must be idempotent and
    not change the document again on a second read of `/openapi.json`.
    """
    app = create_app(settings)
    assert app.openapi() == app.openapi()


def test_committed_snapshot_is_current():
    """
    modama-agent generates its client from the committed `openapi.json`.
    Regenerate it with
    `python -m mgmtinterface_api.openapi_snapshot openapi.json`.
    """
    committed = Path(__file__).parent.parent / "openapi.json"
    assert committed.read_text(encoding="utf-8") == openapi_snapshot.render()
