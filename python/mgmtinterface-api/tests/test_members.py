import pytest
import requests
from conftest import api_client, signed_in_client
from fastapi.testclient import TestClient
from mgmtinterface_api import API_PREFIX, create_app
from mgmtinterface_api.config import Settings
from permissions_client import PermissionsClient

# The owner appears among the members, as it does for real projects: the
# service reads members off the folder's ACL, and the owner usually has an
# entry too.
PROJECT = {
    "relpath": "adhoc/datamanagement",
    "name": "datamanagement",
    "owner": "user1",
    "members": ["user1", "user2", "user3"],
    "ctime": 1_690_000_000.0,
    "mtime": 1_700_000_000.5,
    "gid": 5001,
    "usage_bytes": None,
}

MEMBERS = f"{API_PREFIX}/projects/{PROJECT['name']}/members"


@pytest.fixture
def backend(monkeypatch: pytest.MonkeyPatch) -> dict:
    """
    A permissions service with one project whose membership changes as
    operations are applied. `reject` maps an operation to the message the
    service refuses it with.
    """
    state: dict = {"members": set(PROJECT["members"]), "calls": [], "reject": {}}

    def current() -> dict:
        return {**PROJECT, "members": sorted(state["members"])}

    def projectdetails(self: PermissionsClient, projectname: str, fast: bool = False):
        assert fast, "the members endpoint must never ask for the slow variant"
        if projectname != PROJECT["name"]:
            return {"errno": 1, "message": f'"{projectname}" is not an ad-hoc project.'}
        return {"errno": 0, "project": current(), "datasets": []}

    def change_project_members(
        self: PermissionsClient, projectname: str, op: str, members, user: str
    ):
        state["calls"].append((projectname, op, list(members), user))
        if op in state["reject"]:
            return {"errno": 1, "message": state["reject"][op]}
        if op == "add":
            state["members"] |= set(members)
        else:
            state["members"] -= set(members)
        # The real service serializes with `du` after a change.
        return {"errno": 0, "project": {**current(), "usage_bytes": 4096}}

    monkeypatch.setattr(PermissionsClient, "projectdetails", projectdetails)
    monkeypatch.setattr(
        PermissionsClient, "change_project_members", change_project_members
    )
    return state


@pytest.fixture
def owner(settings: Settings, backend: dict) -> TestClient:
    return signed_in_client(create_app(settings), settings, PROJECT["owner"])


def test_adds_and_removes_whatever_differs_from_the_list(
    owner: TestClient, backend: dict
):
    response = owner.put(MEMBERS, json={"members": ["user2", "user4"]})
    assert response.status_code == 200
    # user4 is new, user3 was dropped, user2 stays. Add before remove, so a
    # refusal leaves the project either unchanged or with the adds applied.
    assert backend["calls"] == [
        (PROJECT["name"], "add", ["user4"], "user1"),
        (PROJECT["name"], "remove", ["user3"], "user1"),
    ]
    body = response.json()
    assert body["members"] == ["user1", "user2", "user4"]
    # The service's `du` on a change is not this endpoint's to report.
    assert body["usage_bytes"] is None


def test_the_owner_is_a_member_whatever_the_list_says(owner: TestClient, backend: dict):
    # Leaving the owner out does not remove them; listing them does not try
    # to add them. Either way, nothing differs from the current members.
    assert owner.put(MEMBERS, json={"members": ["user2", "user3"]}).status_code == 200
    assert (
        owner.put(MEMBERS, json={"members": ["user1", "user2", "user3"]}).status_code
        == 200
    )
    assert backend["calls"] == []


def test_the_same_list_twice_changes_nothing_the_second_time(
    owner: TestClient, backend: dict
):
    """
    The idempotency rule: a client that lost the first answer sends the same
    list again and gets the same project back, with nothing applied twice.
    """
    first = owner.put(MEMBERS, json={"members": ["user2", "user4"]})
    second = owner.put(MEMBERS, json={"members": ["user2", "user4"]})
    assert first.status_code == second.status_code == 200
    assert second.json()["members"] == first.json()["members"]
    assert len(backend["calls"]) == 2


def test_duplicates_in_the_list_are_added_once(owner: TestClient, backend: dict):
    response = owner.put(
        MEMBERS, json={"members": ["user2", "user3", "user4", "user4"]}
    )
    assert response.status_code == 200
    assert backend["calls"] == [(PROJECT["name"], "add", ["user4"], "user1")]


def test_an_empty_list_removes_everyone_but_the_owner(owner: TestClient, backend: dict):
    response = owner.put(MEMBERS, json={"members": []})
    assert response.status_code == 200
    assert backend["calls"] == [
        (PROJECT["name"], "remove", ["user2", "user3"], "user1")
    ]
    assert response.json()["members"] == ["user1"]


def test_only_the_owner_may_change_members(settings: Settings, backend: dict):
    member = signed_in_client(create_app(settings), settings, "user2")
    response = member.put(MEMBERS, json={"members": ["user2", "user4"]})
    assert response.status_code == 403
    assert response.json()["code"] == "not_the_owner"
    assert PROJECT["owner"] in response.json()["message"]
    assert backend["calls"] == []


def test_unknown_project_is_a_404(owner: TestClient, backend: dict):
    response = owner.put(f"{API_PREFIX}/projects/nope/members", json={"members": []})
    assert response.status_code == 404
    assert response.json()["code"] == "project_not_found"


def test_a_refusal_before_any_change_is_a_plain_rejection(
    owner: TestClient, backend: dict
):
    # The service refuses an unknown account the way `pwd.getpwnam` does.
    backend["reject"]["add"] = "'nosuchuser'"
    response = owner.put(MEMBERS, json={"members": ["user2", "user3", "nosuchuser"]})
    assert response.status_code == 400
    assert response.json() == {"code": "members_rejected", "message": "'nosuchuser'"}
    assert backend["members"] == set(PROJECT["members"])


def test_a_refused_remove_alone_is_a_plain_rejection(owner: TestClient, backend: dict):
    backend["reject"]["remove"] = "Encountered errors when walking path"
    response = owner.put(MEMBERS, json={"members": ["user2"]})
    assert response.status_code == 400
    assert response.json()["code"] == "members_rejected"
    assert backend["members"] == set(PROJECT["members"])


def test_a_refusal_after_the_add_says_what_did_change(owner: TestClient, backend: dict):
    """
    Add and remove are two calls to the service and cannot be atomic. When the
    second fails, the project has changed, and the answer has to say so rather
    than claim nothing happened.
    """
    backend["reject"]["remove"] = "Encountered errors when walking path"
    response = owner.put(MEMBERS, json={"members": ["user2", "user4"]})
    assert response.status_code == 400
    body = response.json()
    assert body["code"] == "members_partially_changed"
    assert "user4" in body["message"]
    assert "user3" in body["message"]
    assert "Encountered errors when walking path" in body["message"]
    assert backend["members"] == {"user1", "user2", "user3", "user4"}


def test_a_retry_after_a_partial_change_applies_only_what_is_missing(
    owner: TestClient, backend: dict
):
    backend["reject"]["remove"] = "transient"
    assert owner.put(MEMBERS, json={"members": ["user2", "user4"]}).status_code == 400
    del backend["reject"]["remove"]

    response = owner.put(MEMBERS, json={"members": ["user2", "user4"]})
    assert response.status_code == 200
    # The add is not repeated; only the remove that failed runs.
    assert backend["calls"][-1] == (PROJECT["name"], "remove", ["user3"], "user1")
    assert response.json()["members"] == ["user1", "user2", "user4"]


@pytest.mark.parametrize("body", [{}, {"members": [""]}, {"members": "user2"}])
def test_malformed_bodies_are_rejected_before_any_call(
    owner: TestClient, backend: dict, body: dict
):
    assert owner.put(MEMBERS, json=body).status_code == 422
    assert backend["calls"] == []


def test_requires_a_session(settings: Settings, backend: dict):
    response = api_client(create_app(settings)).put(MEMBERS, json={"members": []})
    assert response.status_code == 401
    assert backend["calls"] == []


def test_is_covered_by_the_origin_check(owner: TestClient, backend: dict):
    response = owner.put(
        MEMBERS, json={"members": []}, headers={"Origin": "https://evil.example"}
    )
    assert response.status_code == 403
    assert response.json()["code"] == "bad_origin"
    assert backend["calls"] == []


def test_reports_an_unreachable_permissions_service(
    owner: TestClient, backend: dict, monkeypatch: pytest.MonkeyPatch
):
    def unreachable(self: PermissionsClient, *args, **kwargs):
        raise requests.ConnectionError("permissions service is down")

    monkeypatch.setattr(PermissionsClient, "change_project_members", unreachable)
    response = owner.put(MEMBERS, json={"members": ["user4"]})
    assert response.status_code == 502
    assert response.json()["code"] == "permissions_unavailable"
