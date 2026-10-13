import pytest
import requests
from conftest import api_client
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
