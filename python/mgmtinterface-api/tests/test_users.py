import pytest
import requests
from conftest import api_client, signed_in_client
from mgmtinterface_api import API_PREFIX, create_app
from mgmtinterface_api.config import Settings
from permissions_client import PermissionsClient

USERS = f"{API_PREFIX}/users"


@pytest.fixture
def backend(monkeypatch: pytest.MonkeyPatch) -> None:
    # The service answers in group-file order, which is whatever order the
    # members were added in.
    monkeypatch.setattr(
        PermissionsClient,
        "list_users",
        lambda self: {"errno": 0, "users": ["weber", "clausen", "shanker"]},
    )


def test_lists_every_user_sorted(settings: Settings, backend: None):
    client = signed_in_client(create_app(settings), settings, "user1")
    response = client.get(USERS)
    assert response.status_code == 200
    assert response.json() == {"users": ["clausen", "shanker", "weber"]}


def test_requires_a_session(settings: Settings, backend: None):
    response = api_client(create_app(settings)).get(USERS)
    assert response.status_code == 401
    assert response.json()["code"] == "not_authenticated"


def test_reports_an_unreachable_permissions_service(
    settings: Settings, monkeypatch: pytest.MonkeyPatch
):
    def unreachable(self: PermissionsClient):
        raise requests.ConnectionError("permissions service is down")

    monkeypatch.setattr(PermissionsClient, "list_users", unreachable)
    client = signed_in_client(create_app(settings), settings, "user1")
    response = client.get(USERS)
    assert response.status_code == 502
    assert response.json()["code"] == "permissions_unavailable"


def test_reports_an_upstream_errno(settings: Settings, monkeypatch: pytest.MonkeyPatch):
    monkeypatch.setattr(
        PermissionsClient,
        "list_users",
        lambda self: {"errno": 1, "message": "group not found"},
    )
    client = signed_in_client(create_app(settings), settings, "user1")
    response = client.get(USERS)
    assert response.status_code == 502
    assert response.json()["code"] == "permissions_unavailable"
