import pytest
import requests
from conftest import api_client, signed_in_client
from fastapi.testclient import TestClient
from mgmtinterface_api import API_PREFIX, create_app
from mgmtinterface_api.config import Settings
from permissions_client import PermissionsClient
from smbdrop_client import ShareClient

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
SESSION = {"name": "create by hand", "immutable": False, "ctime": 1.0, "mtime": 2.0}
UIDS = {"user1": 90_032_240, "user2": 90_032_241}

INGEST_SHARES = f"{API_PREFIX}/ingest-shares"
REQUEST = {"project": PROJECT["name"], "session": SESSION["name"]}

# Where smbdrop sees the session: smb_base + relpath + session name.
DEST_PATH = "/data/adhoc/datamanagement/create by hand"


def make_share(share_id: str, uid: int, dest_path: str = DEST_PATH) -> dict:
    """A share as smbdrop reports it (shape from the test VM capture)."""
    return {
        "resource": None,
        "share_id": share_id,
        "share_name": f"share-{share_id}",
        "dest_path": dest_path,
        "created_timestamp": 1_784_720_092,
        "username": f"share{share_id[:8]}",
        "password": "one-time-secret",
        "uid": uid,
        "gid": PROJECT["gid"],
    }


@pytest.fixture
def backend(monkeypatch: pytest.MonkeyPatch) -> dict:
    """
    The permissions service (one project, one session, two resolvable users)
    and smbdrop (whatever `shares` holds; a create appends a share).
    """
    state: dict = {"shares": {}, "create_calls": [], "delete_calls": []}

    def projectdetails(self: PermissionsClient, projectname: str, fast: bool = False):
        assert fast, "the share endpoints must never ask for the slow variant"
        if projectname != PROJECT["name"]:
            return {"errno": 1, "message": f'"{projectname}" is not an ad-hoc project.'}
        return {"errno": 0, "project": PROJECT, "datasets": [dict(SESSION)]}

    def id_user(self: PermissionsClient, name: str):
        return {"errno": 0, "id": {"uid": UIDS[name]}}

    def list_shares(self: ShareClient):
        return dict(state["shares"])

    def create(self: ShareClient, dest_path: str, uid: int, gid: int, resource=None):
        state["create_calls"].append((dest_path, uid, gid, resource))
        share = make_share(f"new-{len(state['create_calls'])}", uid, dest_path)
        state["shares"][share["share_id"]] = share
        return {"share": share}

    def delete(self: ShareClient, share_id: str):
        state["delete_calls"].append(share_id)
        del state["shares"][share_id]
        return {"status": "ok"}

    monkeypatch.setattr(PermissionsClient, "projectdetails", projectdetails)
    monkeypatch.setattr(PermissionsClient, "id_user", id_user)
    monkeypatch.setattr(ShareClient, "list", list_shares)
    monkeypatch.setattr(ShareClient, "create", create)
    monkeypatch.setattr(ShareClient, "delete", delete)
    return state


@pytest.fixture
def member(settings: Settings, backend: dict) -> TestClient:
    return signed_in_client(create_app(settings), settings, "user2")


def test_opens_a_share_that_writes_as_the_signed_in_user(
    member: TestClient, backend: dict
):
    response = member.post(INGEST_SHARES, json=REQUEST)
    assert response.status_code == 201
    # The share is for the session's path, impersonating the user, with the
    # project's group, like the legacy endpoint.
    assert backend["create_calls"] == [(DEST_PATH, UIDS["user2"], PROJECT["gid"], None)]

    body = response.json()
    assert body["share_id"] == "new-1"
    assert body["dest_path"] == DEST_PATH
    assert body["uid"] == UIDS["user2"]
    # What the legacy API left to the client: which project and session the
    # share is for, and where to connect.
    assert body["project"] == PROJECT["name"]
    assert body["session"] == SESSION["name"]
    assert body["smb_host"] == "moellenstedt.iff.kfa-juelich.de"
    assert body["workgroup"] == "ER-C-INGEST"


def test_host_and_workgroup_come_from_the_settings(settings: Settings, backend: dict):
    configured = Settings(
        **{
            **settings.model_dump(),
            "ingest_smb_host": "ingest.example.org",
            "ingest_workgroup": "LAB",
        }
    )
    member = signed_in_client(create_app(configured), configured, "user2")
    body = member.post(INGEST_SHARES, json=REQUEST).json()
    assert (body["smb_host"], body["workgroup"]) == ("ingest.example.org", "LAB")


def test_repeating_a_create_returns_the_open_share(member: TestClient, backend: dict):
    """
    The idempotency rule: the user's share for this session is returned rather
    than a second one opened. smbdrop itself would happily open another.
    """
    first = member.post(INGEST_SHARES, json=REQUEST)
    second = member.post(INGEST_SHARES, json=REQUEST)
    assert first.status_code == 201
    assert second.status_code == 200
    assert second.json() == first.json()
    assert len(backend["create_calls"]) == 1


def test_an_existing_share_is_matched_on_the_normalized_path(
    member: TestClient, backend: dict
):
    existing = make_share("old", UIDS["user2"], dest_path=f"{DEST_PATH}/")
    backend["shares"][existing["share_id"]] = existing
    response = member.post(INGEST_SHARES, json=REQUEST)
    assert response.status_code == 200
    assert response.json()["share_id"] == "old"
    assert backend["create_calls"] == []


def test_another_users_share_for_the_session_is_not_reused(
    member: TestClient, backend: dict
):
    # Each user ingests under their own identity; user1's open share does not
    # make user2's request a repeat.
    theirs = make_share("theirs", UIDS["user1"])
    backend["shares"][theirs["share_id"]] = theirs
    response = member.post(INGEST_SHARES, json=REQUEST)
    assert response.status_code == 201
    assert response.json()["share_id"] == "new-1"


def test_non_members_get_a_403_before_anything_is_opened(
    settings: Settings, backend: dict
):
    outsider = signed_in_client(create_app(settings), settings, "user3")
    response = outsider.post(INGEST_SHARES, json=REQUEST)
    assert response.status_code == 403
    assert response.json()["code"] == "not_a_member"
    assert backend["create_calls"] == []


def test_unknown_project_is_a_404(member: TestClient, backend: dict):
    response = member.post(INGEST_SHARES, json={**REQUEST, "project": "nope"})
    assert response.status_code == 404
    assert response.json()["code"] == "project_not_found"


def test_unknown_session_is_a_404(member: TestClient, backend: dict):
    response = member.post(INGEST_SHARES, json={**REQUEST, "session": "nope"})
    assert response.status_code == 404
    assert response.json()["code"] == "session_not_found"
    assert backend["create_calls"] == []


def test_create_requires_a_session_cookie(settings: Settings, backend: dict):
    response = api_client(create_app(settings)).post(INGEST_SHARES, json=REQUEST)
    assert response.status_code == 401
    assert backend["create_calls"] == []


def test_reports_an_unreachable_share_service(
    member: TestClient, backend: dict, monkeypatch: pytest.MonkeyPatch
):
    def unreachable(self: ShareClient, *args, **kwargs):
        raise requests.ConnectionError("smbdrop is down")

    monkeypatch.setattr(ShareClient, "list", unreachable)
    response = member.post(INGEST_SHARES, json=REQUEST)
    assert response.status_code == 502
    assert response.json()["code"] == "share_service_unavailable"


def test_reports_a_failed_create(
    member: TestClient, backend: dict, monkeypatch: pytest.MonkeyPatch
):
    # The share client reports a non-2xx from smbdrop as RuntimeError.
    def failed(self: ShareClient, *args, **kwargs):
        raise RuntimeError("something went wrong when creating the share")

    monkeypatch.setattr(ShareClient, "create", failed)
    response = member.post(INGEST_SHARES, json=REQUEST)
    assert response.status_code == 502
    assert response.json()["code"] == "share_service_unavailable"


# --- GET /ingest-shares -------------------------------------------------------


def test_lists_only_the_signed_in_users_shares(member: TestClient, backend: dict):
    mine = make_share("mine", UIDS["user2"])
    theirs = make_share("theirs", UIDS["user1"])
    backend["shares"] = {s["share_id"]: s for s in (mine, theirs)}

    response = member.get(INGEST_SHARES)
    assert response.status_code == 200
    # A list, not the legacy map keyed by share id, and each share says which
    # project and session it is for and where to connect, like a create does.
    assert response.json() == {
        "shares": [
            {
                **mine,
                "project": PROJECT["name"],
                "session": SESSION["name"],
                "smb_host": "moellenstedt.iff.kfa-juelich.de",
                "workgroup": "ER-C-INGEST",
            }
        ]
    }


def test_an_opened_share_appears_in_the_list_as_it_was_answered(
    member: TestClient, backend: dict
):
    opened = member.post(INGEST_SHARES, json=REQUEST).json()
    assert member.get(INGEST_SHARES).json() == {"shares": [opened]}


def test_shares_are_listed_oldest_first(member: TestClient, backend: dict):
    newer = {**make_share("newer", UIDS["user2"]), "created_timestamp": 2_000}
    older = {**make_share("older", UIDS["user2"]), "created_timestamp": 1_000}
    backend["shares"] = {"newer": newer, "older": older}
    ids = [s["share_id"] for s in member.get(INGEST_SHARES).json()["shares"]]
    assert ids == ["older", "newer"]


def test_project_and_session_are_read_off_the_path(member: TestClient, backend: dict):
    # The last two segments of the normalized path, as the legacy
    # `split_smbpath` reads them; smbdrop keeps a trailing slash if given one.
    backend["shares"] = {
        "a": make_share("a", UIDS["user2"], dest_path=f"{DEST_PATH}/"),
        "b": make_share("b", UIDS["user2"], dest_path="/data/adhoc/other/2026 run"),
    }
    shares = member.get(INGEST_SHARES).json()["shares"]
    assert [(s["project"], s["session"]) for s in shares] == [
        (PROJECT["name"], SESSION["name"]),
        ("other", "2026 run"),
    ]


def test_no_shares_is_an_empty_list(member: TestClient, backend: dict):
    assert member.get(INGEST_SHARES).json() == {"shares": []}


def test_list_requires_a_session_cookie(settings: Settings, backend: dict):
    assert api_client(create_app(settings)).get(INGEST_SHARES).status_code == 401


def test_list_reports_an_unreachable_share_service(
    member: TestClient, backend: dict, monkeypatch: pytest.MonkeyPatch
):
    def unreachable(self: ShareClient):
        raise requests.ConnectionError("smbdrop is down")

    monkeypatch.setattr(ShareClient, "list", unreachable)
    response = member.get(INGEST_SHARES)
    assert response.status_code == 502
    assert response.json()["code"] == "share_service_unavailable"


def test_list_reports_an_unreachable_permissions_service(
    member: TestClient, backend: dict, monkeypatch: pytest.MonkeyPatch
):
    # The user's uid is what the list is filtered on; without it there is no
    # safe answer.
    def unreachable(self: PermissionsClient, name: str):
        raise requests.ConnectionError("permissions service is down")

    monkeypatch.setattr(PermissionsClient, "id_user", unreachable)
    response = member.get(INGEST_SHARES)
    assert response.status_code == 502
    assert response.json()["code"] == "permissions_unavailable"


# --- DELETE /ingest-shares/{share_id} -----------------------------------------


def test_closes_the_users_own_share(member: TestClient, backend: dict):
    mine = make_share("mine", UIDS["user2"])
    backend["shares"] = {"mine": mine}
    response = member.delete(f"{INGEST_SHARES}/mine")
    assert response.status_code == 204
    assert response.content == b""
    assert backend["delete_calls"] == ["mine"]
    assert member.get(INGEST_SHARES).json() == {"shares": []}


def test_someone_elses_share_cannot_be_closed(member: TestClient, backend: dict):
    theirs = make_share("theirs", UIDS["user1"])
    backend["shares"] = {"theirs": theirs}
    response = member.delete(f"{INGEST_SHARES}/theirs")
    assert response.status_code == 403
    assert response.json()["code"] == "not_your_share"
    assert backend["delete_calls"] == []


def test_an_unknown_share_is_a_404(member: TestClient, backend: dict):
    response = member.delete(f"{INGEST_SHARES}/nope")
    assert response.status_code == 404
    assert response.json()["code"] == "share_not_found"
    assert backend["delete_calls"] == []


def test_closing_twice_answers_404_the_second_time(member: TestClient, backend: dict):
    # Not idempotent like the creates: once the share is gone there is nothing
    # to say whose it was. The 404 is documented as "treat as closed".
    backend["shares"] = {"mine": make_share("mine", UIDS["user2"])}
    assert member.delete(f"{INGEST_SHARES}/mine").status_code == 204
    assert member.delete(f"{INGEST_SHARES}/mine").status_code == 404


def test_close_requires_a_session_cookie(settings: Settings, backend: dict):
    backend["shares"] = {"mine": make_share("mine", UIDS["user2"])}
    response = api_client(create_app(settings)).delete(f"{INGEST_SHARES}/mine")
    assert response.status_code == 401
    assert backend["delete_calls"] == []


def test_close_is_covered_by_the_origin_check(member: TestClient, backend: dict):
    backend["shares"] = {"mine": make_share("mine", UIDS["user2"])}
    response = member.delete(
        f"{INGEST_SHARES}/mine", headers={"Origin": "https://evil.example"}
    )
    assert response.status_code == 403
    assert response.json()["code"] == "bad_origin"
    assert backend["delete_calls"] == []


def test_close_reports_a_failed_delete(
    member: TestClient, backend: dict, monkeypatch: pytest.MonkeyPatch
):
    backend["shares"] = {"mine": make_share("mine", UIDS["user2"])}

    def failed(self: ShareClient, share_id: str):
        raise RuntimeError("something went wrong when deleting the share")

    monkeypatch.setattr(ShareClient, "delete", failed)
    response = member.delete(f"{INGEST_SHARES}/mine")
    assert response.status_code == 502
    assert response.json()["code"] == "share_service_unavailable"
