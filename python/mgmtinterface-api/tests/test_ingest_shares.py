import json

import pytest
import requests
from conftest import api_client, signed_in_client
from fastapi.testclient import TestClient
from mgmtinterface_api import API_PREFIX, agent_tokens, create_app
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
    state: dict = {
        "shares": {},
        "create_calls": [],
        "created_by": [],
        "delete_calls": [],
    }

    def projectdetails(self: PermissionsClient, projectname: str, fast: bool = False):
        assert fast, "the share endpoints must never ask for the slow variant"
        if projectname != PROJECT["name"]:
            return {"errno": 1, "message": f'"{projectname}" is not an ad-hoc project.'}
        return {"errno": 0, "project": PROJECT, "datasets": [dict(SESSION)]}

    def id_user(self: PermissionsClient, name: str):
        return {"errno": 0, "id": {"uid": UIDS[name]}}

    def list_shares(self: ShareClient):
        return dict(state["shares"])

    def create(
        self: ShareClient,
        dest_path: str,
        uid: int,
        gid: int,
        resource=None,
        created_by=None,
    ):
        state["create_calls"].append((dest_path, uid, gid, resource))
        state["created_by"].append(created_by)
        share = make_share(f"new-{len(state['create_calls'])}", uid, dest_path)
        share["resource"] = resource
        share["created_by"] = created_by
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


def test_the_share_records_who_opened_it(member: TestClient, backend: dict):
    response = member.post(INGEST_SHARES, json=REQUEST)
    assert response.status_code == 201
    assert backend["created_by"] == ["user2"]
    assert response.json()["created_by"] == "user2"


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


# --- POST /ingest-shares with an agent token ----------------------------------

TOKEN = "header.payload.signature"
RESOURCE = "holo"
AGENT_REQUEST = {**REQUEST, "resource": RESOURCE}
AGENT_HEADERS = {"X-Agent-Token": TOKEN}
VERIFIED = {"VerifiedAgent": {"agent_id": f"x-modama-agent://{RESOURCE}#agent-1"}}


@pytest.fixture
def token_service(monkeypatch: pytest.MonkeyPatch) -> dict:
    """
    smbdrop's token service, in place of `requests.put`: `answers` maps a
    token to what `/verify/` says about it, and any other token is
    `"Invalid"`, as the service answers for a token it did not sign or that
    has expired. `status`, `body` and `error` make it misbehave.
    """
    state: dict = {
        "answers": {TOKEN: VERIFIED},
        "calls": [],
        "status": 200,
        "body": None,
        "error": None,
    }

    def put(url: str, data: bytes, verify: str, timeout: float) -> requests.Response:
        if state["error"] is not None:
            raise state["error"]
        state["calls"].append((url, data, verify))
        answer = state["answers"].get(data.decode(), "Invalid")
        response = requests.Response()
        response.status_code = state["status"]
        response._content = state["body"] or json.dumps(answer).encode()
        return response

    monkeypatch.setattr(agent_tokens.requests, "put", put)
    return state


def test_an_agent_token_binds_the_share_to_its_instrument(
    member: TestClient, backend: dict, token_service: dict
):
    response = member.post(INGEST_SHARES, json=AGENT_REQUEST, headers=AGENT_HEADERS)
    assert response.status_code == 201
    # Still opened as the signed-in user; the token only adds the binding.
    assert backend["create_calls"] == [
        (DEST_PATH, UIDS["user2"], PROJECT["gid"], RESOURCE)
    ]
    assert response.json()["resource"] == RESOURCE


def test_the_token_is_checked_with_the_configured_service(
    settings: Settings, backend: dict, token_service: dict
):
    configured = Settings(
        **{
            **settings.model_dump(),
            "token_api": "https://tokens.example.org:5053",
            "token_ca_cert": "/etc/ca.pem",
        }
    )
    member = signed_in_client(create_app(configured), configured, "user2")
    member.post(INGEST_SHARES, json=AGENT_REQUEST, headers=AGENT_HEADERS)
    # Trusting the deployment's CA, not the system's.
    assert token_service["calls"] == [
        ("https://tokens.example.org:5053/verify/", TOKEN.encode(), "/etc/ca.pem")
    ]


def test_a_token_does_not_replace_the_session(
    settings: Settings, backend: dict, token_service: dict
):
    # The two pieces answer different questions: without a session nobody is
    # signed in, whatever instrument the token names.
    client = api_client(create_app(settings))
    response = client.post(INGEST_SHARES, json=AGENT_REQUEST, headers=AGENT_HEADERS)
    assert response.status_code == 401
    assert token_service["calls"] == []
    assert backend["create_calls"] == []


def test_a_token_does_not_make_a_non_member_a_member(
    settings: Settings, backend: dict, token_service: dict
):
    outsider = signed_in_client(create_app(settings), settings, "user3")
    response = outsider.post(INGEST_SHARES, json=AGENT_REQUEST, headers=AGENT_HEADERS)
    assert response.status_code == 403
    assert response.json()["code"] == "not_a_member"
    assert backend["create_calls"] == []


def test_a_repeat_with_a_token_returns_the_bound_share(
    member: TestClient, backend: dict, token_service: dict
):
    first = member.post(INGEST_SHARES, json=AGENT_REQUEST, headers=AGENT_HEADERS)
    second = member.post(INGEST_SHARES, json=AGENT_REQUEST, headers=AGENT_HEADERS)
    assert (first.status_code, second.status_code) == (201, 200)
    assert second.json() == first.json()
    assert len(backend["create_calls"]) == 1


@pytest.mark.parametrize("bound_to", [None, "tem"])
def test_a_share_not_bound_to_the_instrument_is_not_a_repeat(
    member: TestClient, backend: dict, token_service: dict, bound_to: str | None
):
    # smbdrop offers the agent only shares bound to its instrument; returning
    # this one would leave the agent waiting for a share that never comes.
    existing = {**make_share("old", UIDS["user2"]), "resource": bound_to}
    backend["shares"][existing["share_id"]] = existing
    response = member.post(INGEST_SHARES, json=AGENT_REQUEST, headers=AGENT_HEADERS)
    assert response.status_code == 201
    assert response.json()["resource"] == RESOURCE


def test_without_a_token_a_bound_share_still_counts_as_a_repeat(
    member: TestClient, backend: dict, token_service: dict
):
    # Unchanged for the web interface: any share the user has for the session.
    existing = {**make_share("old", UIDS["user2"]), "resource": RESOURCE}
    backend["shares"][existing["share_id"]] = existing
    response = member.post(INGEST_SHARES, json=REQUEST)
    assert response.status_code == 200
    assert response.json()["share_id"] == "old"
    assert token_service["calls"] == []


def test_a_token_for_another_instrument_is_refused(
    member: TestClient, backend: dict, token_service: dict
):
    response = member.post(
        INGEST_SHARES, json={**REQUEST, "resource": "tem"}, headers=AGENT_HEADERS
    )
    assert response.status_code == 403
    assert response.json()["code"] == "resource_mismatch"
    assert backend["create_calls"] == []


@pytest.mark.parametrize(
    "answer",
    [
        "Invalid",
        {"VerifiedAgent": {}},
        {"VerifiedAgent": {"agent_id": f"https://{RESOURCE}#agent-1"}},
        {"VerifiedAgent": {"agent_id": "x-modama-agent:#agent-1"}},
        ["VerifiedAgent"],
    ],
)
def test_a_token_the_service_does_not_vouch_for_is_a_403(
    member: TestClient, backend: dict, token_service: dict, answer: object
):
    # 403, not 401: the session is fine, and a 401 would send the agent to log
    # in again when what it needs is a fresh token.
    token_service["answers"][TOKEN] = answer
    response = member.post(INGEST_SHARES, json=AGENT_REQUEST, headers=AGENT_HEADERS)
    assert response.status_code == 403
    assert response.json()["code"] == "invalid_agent_token"
    assert backend["create_calls"] == []


@pytest.mark.parametrize(
    ("body", "headers"),
    [(REQUEST, AGENT_HEADERS), (AGENT_REQUEST, {})],
    ids=["token without resource", "resource without token"],
)
def test_token_and_resource_come_together(
    member: TestClient, backend: dict, token_service: dict, body: dict, headers: dict
):
    response = member.post(INGEST_SHARES, json=body, headers=headers)
    assert response.status_code == 400
    assert response.json()["code"] == "agent_binding_incomplete"
    assert token_service["calls"] == []
    assert backend["create_calls"] == []


@pytest.mark.parametrize(
    "misbehaviour",
    [
        {"error": requests.ConnectionError("the token service is down")},
        # What the test VM answers: Python 3.13 rejects smbdrop's certificate
        # there, which was issued without an Authority Key Identifier.
        {
            "error": requests.exceptions.SSLError(
                "certificate verify failed: Missing Authority Key Identifier"
            )
        },
        {"status": 500},
        {"body": b"<html>Bad Gateway</html>"},
    ],
    ids=["unreachable", "rejected certificate", "error status", "no JSON"],
)
def test_reports_an_unusable_token_service(
    member: TestClient, backend: dict, token_service: dict, misbehaviour: dict
):
    token_service.update(misbehaviour)
    response = member.post(INGEST_SHARES, json=AGENT_REQUEST, headers=AGENT_HEADERS)
    assert response.status_code == 502
    assert response.json()["code"] == "token_service_unavailable"
    assert backend["create_calls"] == []


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
                # smbdrop reports no creator for shares opened before it
                # recorded one:
                "created_by": None,
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


def test_the_listed_share_says_who_opened_it(member: TestClient, backend: dict):
    mine = {**make_share("mine", UIDS["user2"]), "created_by": "user2"}
    backend["shares"] = {"mine": mine}
    [listed] = member.get(INGEST_SHARES).json()["shares"]
    assert listed["created_by"] == "user2"


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
