import pytest
import requests
from conftest import api_client, signed_in_client
from fastapi.testclient import TestClient
from mgmtinterface_api import API_PREFIX, create_app
from mgmtinterface_api.config import Settings
from permissions_client import PermissionsClient

PROJECT = {
    "relpath": "adhoc/datamanagement",
    "name": "datamanagement",
    "owner": "user1",
    "members": ["user1", "user2"],
    "ctime": 1_690_000_000.0,
    "mtime": 1_700_000_000.5,
    "gid": 5001,
    "usage_bytes": None,
}

EXISTING = {
    "name": "2026-10-15-alignment",
    "immutable": True,
    "ctime": 1_760_000_000.0,
    "mtime": 1_760_003_600.0,
}

SESSIONS = f"{API_PREFIX}/projects/{PROJECT['name']}/sessions"


@pytest.fixture
def backend(monkeypatch: pytest.MonkeyPatch) -> dict:
    """
    A permissions service with one project that has one session. Creating a
    session appends it, unless `reject` is set, in which case the service
    answers the way it does when `mkdir` fails.
    """
    state: dict = {"datasets": [dict(EXISTING)], "create_calls": [], "reject": None}

    def projectdetails(self: PermissionsClient, projectname: str, fast: bool = False):
        assert fast, "the create endpoints must never ask for the slow variant"
        if projectname != PROJECT["name"]:
            return {"errno": 1, "message": f'"{projectname}" is not an ad-hoc project.'}
        return {
            "errno": 0,
            "project": PROJECT,
            "datasets": [dict(d) for d in state["datasets"]],
        }

    def create_dataset(self: PermissionsClient, projectname: str, name: str, user: str):
        state["create_calls"].append((projectname, name, user))
        if state["reject"] is not None:
            return {"errno": 1, "message": state["reject"]}
        state["datasets"].append(
            {"name": name, "immutable": False, "ctime": 1.0, "mtime": 2.0}
        )
        return {"errno": 0}

    monkeypatch.setattr(PermissionsClient, "projectdetails", projectdetails)
    monkeypatch.setattr(PermissionsClient, "create_dataset", create_dataset)
    return state


@pytest.fixture
def member(settings: Settings, backend: dict) -> TestClient:
    return signed_in_client(create_app(settings), settings, "user2")


def test_creates_a_session_as_the_signed_in_user(member: TestClient, backend: dict):
    response = member.post(SESSIONS, json={"name": "2026-10-16-run 1"})
    assert response.status_code == 201
    # The service answers a create with nothing but errno; the timestamps and
    # the sealed state come from reading the project back.
    assert response.json() == {
        "name": "2026-10-16-run 1",
        "immutable": False,
        "ctime": 1.0,
        "mtime": 2.0,
    }
    assert backend["create_calls"] == [(PROJECT["name"], "2026-10-16-run 1", "user2")]


def test_the_owner_can_create_sessions_too(settings: Settings, backend: dict):
    owner = signed_in_client(create_app(settings), settings, PROJECT["owner"])
    assert owner.post(SESSIONS, json={"name": "run"}).status_code == 201


def test_repeating_a_create_returns_the_existing_session(
    member: TestClient, backend: dict
):
    """The idempotency rule; see the project tests for why."""
    response = member.post(SESSIONS, json={"name": EXISTING["name"]})
    assert response.status_code == 200
    assert response.json() == EXISTING
    # Nothing was created, and the service did not have to refuse anything.
    assert backend["create_calls"] == []


def test_a_session_created_by_a_concurrent_request_is_not_an_error(
    member: TestClient, backend: dict, monkeypatch: pytest.MonkeyPatch
):
    """
    Two retries of the same request can race: both see no session, one of the
    `mkdir`s fails. The refusal is then re-checked against the project.
    """

    def created_meanwhile(
        self: PermissionsClient, projectname: str, name: str, user: str
    ):
        # The other request won: the folder is there by the time this `mkdir`
        # runs, so the service refuses, but the project now lists the session.
        backend["datasets"].append(
            {"name": name, "immutable": False, "ctime": 3.0, "mtime": 4.0}
        )
        return {"errno": 1, "message": "mkdir: cannot create directory: File exists"}

    monkeypatch.setattr(PermissionsClient, "create_dataset", created_meanwhile)
    response = member.post(SESSIONS, json={"name": "raced"})
    assert response.status_code == 200
    assert response.json()["name"] == "raced"


def test_a_refusal_is_reported_with_the_services_message(
    member: TestClient, backend: dict
):
    backend["reject"] = "mkdir: cannot create directory: Permission denied"
    response = member.post(SESSIONS, json={"name": "run"})
    assert response.status_code == 400
    assert response.json() == {
        "code": "session_rejected",
        "message": "mkdir: cannot create directory: Permission denied",
    }


def test_non_members_get_a_403_before_anything_is_created(
    settings: Settings, backend: dict
):
    outsider = signed_in_client(create_app(settings), settings, "user3")
    response = outsider.post(SESSIONS, json={"name": "run"})
    assert response.status_code == 403
    assert response.json()["code"] == "not_a_member"
    assert backend["create_calls"] == []


def test_unknown_project_is_a_404(member: TestClient, backend: dict):
    response = member.post(f"{API_PREFIX}/projects/nope/sessions", json={"name": "run"})
    assert response.status_code == 404
    assert response.json()["code"] == "project_not_found"
    assert backend["create_calls"] == []


@pytest.mark.parametrize(
    "name",
    [
        "",
        "a" * 256,
        "with/slash",
        "tab\there",
        "umlaut-ä",
        ".",
        "..",
    ],
)
def test_invalid_session_names_are_rejected_before_any_call(
    member: TestClient, backend: dict, name: str
):
    assert member.post(SESSIONS, json={"name": name}).status_code == 422
    assert backend["create_calls"] == []


def test_session_names_may_contain_spaces_and_dots(member: TestClient, backend: dict):
    # Session names are folder names that people type; the legacy UI's
    # default is a date prefix, and real projects have names like
    # "2022-03-22 my first session" and "v1.2 calibration".
    assert member.post(SESSIONS, json={"name": "v1.2 calibration"}).status_code == 201


def test_create_requires_a_session_cookie(settings: Settings, backend: dict):
    response = api_client(create_app(settings)).post(SESSIONS, json={"name": "run"})
    assert response.status_code == 401
    assert backend["create_calls"] == []


def test_create_reports_an_unreachable_permissions_service(
    settings: Settings, monkeypatch: pytest.MonkeyPatch
):
    def unreachable(self: PermissionsClient, *args, **kwargs):
        raise requests.ConnectionError("permissions service is down")

    monkeypatch.setattr(PermissionsClient, "projectdetails", unreachable)
    member = signed_in_client(create_app(settings), settings, "user2")
    response = member.post(SESSIONS, json={"name": "run"})
    assert response.status_code == 502
    assert response.json()["code"] == "permissions_unavailable"
