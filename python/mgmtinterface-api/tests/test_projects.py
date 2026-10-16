import pytest
import requests
from conftest import api_client, signed_in_client
from fastapi.testclient import TestClient
from mgmtinterface_api import API_PREFIX, create_app
from mgmtinterface_api.config import Settings
from permissions_client import PermissionsClient

PROJECTS = f"{API_PREFIX}/projects"

FIXTURE = {
    "relpath": "adhoc/datamanagement",
    "name": "datamanagement",
    "owner": "user1",
    "members": ["user1", "user2"],
    "ctime": 1_690_000_000.0,
    "mtime": 1_700_000_000.5,
    "gid": 5001,
    # Null in the list view; the permissions service only computes usage for a
    # single project.
    "usage_bytes": None,
}


@pytest.fixture
def signed_in(settings: Settings, monkeypatch: pytest.MonkeyPatch) -> TestClient:
    monkeypatch.setattr(
        PermissionsClient,
        "list_projects",
        lambda self: {"errno": 0, "projects": [FIXTURE]},
    )
    app = create_app(settings)
    client = api_client(app)
    client.cookies.set(
        settings.effective_cookie_name, app.state.session_codec.issue("user1")
    )
    return client


def test_lists_projects(signed_in: TestClient):
    response = signed_in.get(PROJECTS)
    assert response.status_code == 200
    assert response.json() == {"projects": [FIXTURE]}


def test_requires_a_session(settings: Settings):
    response = api_client(create_app(settings)).get(PROJECTS)
    assert response.status_code == 401
    assert response.json()["code"] == "not_authenticated"


def test_reports_an_unreachable_permissions_service(
    settings: Settings, monkeypatch: pytest.MonkeyPatch
):
    def unreachable(self: PermissionsClient):
        raise requests.ConnectionError("permissions service is down")

    monkeypatch.setattr(PermissionsClient, "list_projects", unreachable)
    app = create_app(settings)
    client = api_client(app)
    client.cookies.set(
        settings.effective_cookie_name, app.state.session_codec.issue("user1")
    )

    response = client.get(PROJECTS)
    assert response.status_code == 502
    assert response.json()["code"] == "permissions_unavailable"


def test_reports_an_upstream_errno(settings: Settings, monkeypatch: pytest.MonkeyPatch):
    # The legacy API would have passed `errno` straight through with HTTP 200;
    # here a failure has to be a failure status.
    monkeypatch.setattr(
        PermissionsClient,
        "list_projects",
        lambda self: {"errno": 1, "message": "backend exploded"},
    )
    app = create_app(settings)
    client = api_client(app)
    client.cookies.set(
        settings.effective_cookie_name, app.state.session_codec.issue("user1")
    )

    response = client.get(PROJECTS)
    assert response.status_code == 502
    assert response.json()["code"] == "permissions_unavailable"


def test_response_is_not_wrapped_in_an_errno_envelope(signed_in: TestClient):
    assert "errno" not in signed_in.get(PROJECTS).json()


# --- POST /projects -----------------------------------------------------------

NEW_NAME = "ghost_imaging_2026"


@pytest.fixture
def backend(monkeypatch: pytest.MonkeyPatch) -> dict:
    """
    A permissions service with one existing project (`FIXTURE`, owned by
    user1). Creating any other name succeeds; creating the existing name is
    refused the way the real service refuses it.
    """
    state: dict = {"create_calls": [], "reject": None}

    def create_project(self: PermissionsClient, name: str, owner: str, members):
        state["create_calls"].append((name, owner, list(members)))
        if state["reject"] is not None:
            return {"errno": 1, "message": state["reject"]}
        if name == FIXTURE["name"]:
            return {"errno": 1, "message": "File or folder exists in adhoc folder."}
        project = {**FIXTURE, "name": name, "relpath": f"adhoc/{name}"}
        project["owner"] = owner
        project["members"] = [owner, *members]
        project["usage_bytes"] = 0
        return {"errno": 0, "project": project}

    def projectdetails(self: PermissionsClient, projectname: str, fast: bool = False):
        assert fast, "the create endpoints must never ask for the slow variant"
        if projectname != FIXTURE["name"]:
            return {"errno": 1, "message": f'"{projectname}" is not an ad-hoc project.'}
        return {"errno": 0, "project": FIXTURE, "datasets": []}

    monkeypatch.setattr(PermissionsClient, "create_project", create_project)
    monkeypatch.setattr(PermissionsClient, "projectdetails", projectdetails)
    return state


def test_creates_a_project_owned_by_the_signed_in_user(
    settings: Settings, backend: dict
):
    client = signed_in_client(create_app(settings), settings, "user1")
    response = client.post(PROJECTS, json={"name": NEW_NAME, "members": ["user2"]})
    assert response.status_code == 201
    assert response.json()["name"] == NEW_NAME
    assert response.json()["owner"] == "user1"
    # Null on both outcomes, like the list: the service computes a size on
    # create but not on the lookup a repeat answers from, and the two bodies
    # should not differ by which path was taken.
    assert response.json()["usage_bytes"] is None
    # No `owner` in the request: the legacy API took one and then insisted it
    # equal the authenticated user. Here the session is the only source.
    assert backend["create_calls"] == [(NEW_NAME, "user1", ["user2"])]


def test_members_are_deduplicated_and_never_include_the_owner(
    settings: Settings, backend: dict
):
    client = signed_in_client(create_app(settings), settings, "user1")
    response = client.post(
        PROJECTS, json={"name": NEW_NAME, "members": ["user2", "user1", "user2"]}
    )
    assert response.status_code == 201
    assert backend["create_calls"] == [(NEW_NAME, "user1", ["user2"])]


def test_members_default_to_nobody(settings: Settings, backend: dict):
    client = signed_in_client(create_app(settings), settings, "user1")
    assert client.post(PROJECTS, json={"name": NEW_NAME}).status_code == 201
    assert backend["create_calls"] == [(NEW_NAME, "user1", [])]


def test_repeating_a_create_returns_the_existing_project(
    settings: Settings, backend: dict
):
    """
    The idempotency rule: the same request twice is not an error, because the
    agent retries on flaky networks and may never have seen the first answer.
    """
    client = signed_in_client(create_app(settings), settings, FIXTURE["owner"])
    response = client.post(PROJECTS, json={"name": FIXTURE["name"], "members": []})
    assert response.status_code == 200
    assert response.json() == FIXTURE


def test_existing_project_is_returned_unchanged(settings: Settings, backend: dict):
    # A different member list on the repeat does not turn the create into a
    # membership change; that is a separate operation.
    client = signed_in_client(create_app(settings), settings, FIXTURE["owner"])
    response = client.post(
        PROJECTS, json={"name": FIXTURE["name"], "members": ["user99"]}
    )
    assert response.status_code == 200
    assert response.json()["members"] == FIXTURE["members"]


def test_someone_elses_project_is_a_conflict(settings: Settings, backend: dict):
    client = signed_in_client(create_app(settings), settings, "user2")
    response = client.post(PROJECTS, json={"name": FIXTURE["name"]})
    assert response.status_code == 409
    assert response.json()["code"] == "project_exists"
    assert FIXTURE["owner"] in response.json()["message"]


def test_a_refusal_for_a_name_that_does_not_exist_is_reported(
    settings: Settings, backend: dict
):
    # The service refuses for reasons other than "exists" too (a name that is a
    # Unix group, an account outside the ad-hoc group). Its message is the
    # explanation, so it is passed on.
    backend["reject"] = 'User "user1" is not in the group "testerc".'
    client = signed_in_client(create_app(settings), settings, "user1")
    response = client.post(PROJECTS, json={"name": NEW_NAME})
    assert response.status_code == 400
    assert response.json() == {
        "code": "project_rejected",
        "message": 'User "user1" is not in the group "testerc".',
    }


@pytest.mark.parametrize(
    "name",
    [
        "ab",  # too short
        "a" * 33,  # too long
        "Ghost",  # uppercase
        "1ghost",  # must start with a letter
        "ghost/imaging",  # not a folder name
        "ghost imaging",  # no spaces
        "_ghost",
    ],
)
def test_invalid_project_names_are_rejected_before_any_call(
    settings: Settings, backend: dict, name: str
):
    client = signed_in_client(create_app(settings), settings, "user1")
    assert client.post(PROJECTS, json={"name": name}).status_code == 422
    assert backend["create_calls"] == []


def test_create_requires_a_session(settings: Settings, backend: dict):
    response = api_client(create_app(settings)).post(PROJECTS, json={"name": NEW_NAME})
    assert response.status_code == 401
    assert backend["create_calls"] == []


def test_create_is_covered_by_the_origin_check(settings: Settings, backend: dict):
    # The CSRF check is registered app-wide; this pins that a new router is
    # covered without declaring anything.
    client = signed_in_client(create_app(settings), settings, "user1")
    response = client.post(
        PROJECTS, json={"name": NEW_NAME}, headers={"Origin": "https://evil.example"}
    )
    assert response.status_code == 403
    assert backend["create_calls"] == []


def test_create_reports_an_unreachable_permissions_service(
    settings: Settings, monkeypatch: pytest.MonkeyPatch
):
    def unreachable(self: PermissionsClient, *args, **kwargs):
        raise requests.ConnectionError("permissions service is down")

    monkeypatch.setattr(PermissionsClient, "create_project", unreachable)
    client = signed_in_client(create_app(settings), settings, "user1")
    response = client.post(PROJECTS, json={"name": NEW_NAME})
    assert response.status_code == 502
    assert response.json()["code"] == "permissions_unavailable"
