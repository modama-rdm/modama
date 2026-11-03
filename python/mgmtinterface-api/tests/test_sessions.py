from urllib.parse import quote

import pytest
import requests
from conftest import api_client, signed_in_client
from fastapi.testclient import TestClient
from mgmtinterface_api import API_PREFIX, create_app
from mgmtinterface_api.config import Settings
from mgmtinterface_api.descriptions import NotAPlainFile
from mgmtinterface_api.errors import APIError
from mgmtinterface_api.webdav import NoAccess, NotFound
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
    "description": None,
}

EXISTING = {
    "name": "2026-10-15-alignment",
    "immutable": True,
    "ctime": 1_760_000_000.0,
    "mtime": 1_760_003_600.0,
    "description": "Alignment before the holography series.",
}
# An open session, with a space in its name, as people type them.
OPEN = {
    "name": "2026-10-16 run 1",
    "immutable": False,
    "ctime": 1_760_100_000.0,
    "mtime": 1_760_100_000.0,
    "description": None,
}

SESSIONS = f"{API_PREFIX}/projects/{PROJECT['name']}/sessions"


def seal_url(session: str, project: str = PROJECT["name"]) -> str:
    return f"{API_PREFIX}/projects/{project}/sessions/{quote(session, safe='')}/seal"


def description_url(session: str, project: str = PROJECT["name"]) -> str:
    return f"{API_PREFIX}/projects/{project}/sessions/{quote(session, safe='')}/description"


@pytest.fixture
def backend(monkeypatch: pytest.MonkeyPatch) -> dict:
    """
    A permissions service with one project that has a sealed and an open
    session. Creating a session appends it, unless `reject` is set, in which
    case the service answers the way it does when `mkdir` fails. Sealing
    flips `immutable`, unless `seal_reject` is set.

    README.md writes, which go over WebDAV, are recorded in `writes` as
    `(user, folder, text)`, text None for a removal, and fail with
    `write_error` when it is set. A write shows in the session read back, as
    the permissions service would read the file.
    """
    state: dict = {
        "datasets": [dict(EXISTING), dict(OPEN)],
        "create_calls": [],
        "reject": None,
        "seal_calls": [],
        "seal_reject": None,
        "writes": [],
        "write_error": None,
    }

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
            {
                "name": name,
                "immutable": False,
                "ctime": 1.0,
                "mtime": 2.0,
                "description": None,
            }
        )
        return {"errno": 0}

    def write_description(
        settings: Settings, username: str, folder: str, text: str | None
    ):
        state["writes"].append((username, folder, text))
        if state["write_error"] is not None:
            raise state["write_error"]
        for dataset in state["datasets"]:
            if folder == f"/{PROJECT['relpath']}/{dataset['name']}":
                dataset["description"] = text

    def seal_dataset(self: PermissionsClient, projectname: str, name: str, user: str):
        state["seal_calls"].append((projectname, name, user))
        if state["seal_reject"] is not None:
            return {"errno": 1, "message": state["seal_reject"]}
        for dataset in state["datasets"]:
            if dataset["name"] == name:
                dataset["immutable"] = True
                dataset["mtime"] += 60.0
        return {"errno": 0}

    monkeypatch.setattr(PermissionsClient, "projectdetails", projectdetails)
    monkeypatch.setattr(PermissionsClient, "create_dataset", create_dataset)
    monkeypatch.setattr(PermissionsClient, "seal_dataset", seal_dataset)
    monkeypatch.setattr(
        "mgmtinterface_api.descriptions.write_description", write_description
    )
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
        "description": None,
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
        self: PermissionsClient,
        projectname: str,
        name: str,
        user: str,
        description: str | None = None,
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


# --- Descriptions on create ----------------------------------------------------


def test_a_description_is_written_normalized_as_the_signed_in_user(
    member: TestClient, backend: dict
):
    response = member.post(
        SESSIONS, json={"name": "run 2", "description": "\n# Run 2\r\nTitan, 300 kV \n"}
    )
    assert response.status_code == 201
    # Into the new folder, after the permissions service created it.
    assert backend["writes"] == [
        ("user2", "/adhoc/datamanagement/run 2", "# Run 2\nTitan, 300 kV")
    ]
    # Read back from the project, as the timestamps are.
    assert response.json()["description"] == "# Run 2\nTitan, 300 kV"


@pytest.mark.parametrize(
    "body", [{"name": "run 2"}, {"name": "run 2", "description": " "}]
)
def test_without_a_description_no_file_is_written(
    member: TestClient, backend: dict, body: dict
):
    response = member.post(SESSIONS, json=body)
    assert response.status_code == 201
    assert backend["writes"] == []
    assert response.json()["description"] is None


def test_a_longer_description_is_rejected_before_any_call(
    member: TestClient, backend: dict
):
    response = member.post(SESSIONS, json={"name": "run", "description": "a" * 10_001})
    assert response.status_code == 422
    assert backend["create_calls"] == []
    assert backend["writes"] == []


def test_a_description_that_cannot_be_written_leaves_the_session(
    member: TestClient, backend: dict
):
    backend["write_error"] = APIError(502, "files_unavailable", "down")
    response = member.post(SESSIONS, json={"name": "run 2", "description": "x"})
    assert response.status_code == 502
    assert response.json()["code"] == "description_not_saved"
    assert "'run 2'" in response.json()["message"]
    # Created, and not taken back: a retry gets it with a 200.
    assert backend["create_calls"] == [(PROJECT["name"], "run 2", "user2")]
    assert member.post(SESSIONS, json={"name": "run 2"}).status_code == 200


@pytest.mark.parametrize("name", ["README.md", "readme.md", "ReadMe.MD"])
def test_a_session_may_not_take_the_readmes_name(
    member: TestClient, backend: dict, name: str
):
    # It would collide with the project's own README.md.
    assert member.post(SESSIONS, json={"name": name}).status_code == 422
    assert backend["create_calls"] == []


def test_repeating_a_create_does_not_apply_the_description(
    member: TestClient, backend: dict
):
    response = member.post(
        SESSIONS, json={"name": EXISTING["name"], "description": "something else"}
    )
    assert response.status_code == 200
    assert response.json()["description"] == EXISTING["description"]
    assert backend["writes"] == []


# --- POST /projects/{project}/sessions/{session}/seal -------------------------


def test_seals_an_open_session_as_the_signed_in_user(member: TestClient, backend: dict):
    response = member.post(seal_url(OPEN["name"]))
    assert response.status_code == 200
    # The service answers with nothing but errno; the sealed state and the
    # new mtime come from reading the project back.
    assert response.json() == {**OPEN, "immutable": True, "mtime": OPEN["mtime"] + 60.0}
    # The session name travels URL-encoded in the path and arrives as typed.
    assert backend["seal_calls"] == [(PROJECT["name"], OPEN["name"], "user2")]


def test_the_owner_can_seal_too(settings: Settings, backend: dict):
    owner = signed_in_client(create_app(settings), settings, PROJECT["owner"])
    assert owner.post(seal_url(OPEN["name"])).status_code == 200


def test_sealing_a_sealed_session_is_not_an_error(member: TestClient, backend: dict):
    """
    Idempotent: a retry after a lost answer finds the session sealed and gets
    it back as it is, without the service being asked to seal it again.
    """
    response = member.post(seal_url(EXISTING["name"]))
    assert response.status_code == 200
    assert response.json() == EXISTING
    assert backend["seal_calls"] == []


def test_non_members_get_a_403_before_anything_is_sealed(
    settings: Settings, backend: dict
):
    outsider = signed_in_client(create_app(settings), settings, "user3")
    response = outsider.post(seal_url(OPEN["name"]))
    assert response.status_code == 403
    assert response.json()["code"] == "not_a_member"
    assert backend["seal_calls"] == []


def test_seal_of_an_unknown_project_is_a_404(member: TestClient, backend: dict):
    response = member.post(seal_url(OPEN["name"], project="nope"))
    assert response.status_code == 404
    assert response.json()["code"] == "project_not_found"


def test_seal_of_an_unknown_session_is_a_404(member: TestClient, backend: dict):
    response = member.post(seal_url("nope"))
    assert response.status_code == 404
    assert response.json()["code"] == "session_not_found"
    assert backend["seal_calls"] == []


def test_a_seal_refusal_is_reported_with_the_services_message(
    member: TestClient, backend: dict
):
    backend["seal_reject"] = "Raw folder does not exist."
    response = member.post(seal_url(OPEN["name"]))
    assert response.status_code == 400
    assert response.json() == {
        "code": "seal_rejected",
        "message": "Raw folder does not exist.",
    }


def test_seal_requires_a_session_cookie(settings: Settings, backend: dict):
    response = api_client(create_app(settings)).post(seal_url(OPEN["name"]))
    assert response.status_code == 401
    assert backend["seal_calls"] == []


def test_seal_is_covered_by_the_origin_check(member: TestClient, backend: dict):
    response = member.post(
        seal_url(OPEN["name"]), headers={"Origin": "https://evil.example"}
    )
    assert response.status_code == 403
    assert response.json()["code"] == "bad_origin"
    assert backend["seal_calls"] == []


def test_seal_reports_an_unreachable_permissions_service(
    member: TestClient, backend: dict, monkeypatch: pytest.MonkeyPatch
):
    def unreachable(self: PermissionsClient, *args, **kwargs):
        raise requests.ConnectionError("permissions service is down")

    monkeypatch.setattr(PermissionsClient, "seal_dataset", unreachable)
    response = member.post(seal_url(OPEN["name"]))
    assert response.status_code == 502
    assert response.json()["code"] == "permissions_unavailable"


# --- PUT /projects/{project}/sessions/{session}/description -------------------


@pytest.mark.parametrize("user", [PROJECT["owner"], "user2"])
def test_owner_and_members_may_try_to_set_a_session_description(
    settings: Settings, backend: dict, user: str
):
    # Whether the file system lets them is the WebDAV server's answer.
    client = signed_in_client(create_app(settings), settings, user)
    response = client.put(
        description_url(OPEN["name"]), json={"description": " First run\r\n"}
    )
    assert response.status_code == 200
    # Read back from the project; the name travels URL-encoded and arrives as typed.
    assert response.json() == {**OPEN, "description": "First run"}
    assert backend["writes"] == [
        (user, f"/adhoc/datamanagement/{OPEN['name']}", "First run")
    ]


def test_a_sealed_session_can_be_described(member: TestClient, backend: dict):
    # Sealing covers `raw/` only; the README sits next to it, and is often
    # written once the data is in.
    response = member.put(description_url(EXISTING["name"]), json={"description": "x"})
    assert response.status_code == 200
    assert response.json()["immutable"] is True
    assert response.json()["description"] == "x"
    assert backend["writes"] == [
        ("user2", f"/adhoc/datamanagement/{EXISTING['name']}", "x")
    ]


def test_an_empty_session_description_removes_it(member: TestClient, backend: dict):
    backend["datasets"][1]["description"] = "old"
    response = member.put(description_url(OPEN["name"]), json={"description": ""})
    assert response.status_code == 200
    assert response.json()["description"] is None
    assert backend["writes"] == [
        ("user2", f"/adhoc/datamanagement/{OPEN['name']}", None)
    ]


def test_others_may_not_set_a_session_description(settings: Settings, backend: dict):
    outsider = signed_in_client(create_app(settings), settings, "user3")
    response = outsider.put(description_url(OPEN["name"]), json={"description": "x"})
    assert response.status_code == 403
    assert response.json()["code"] == "not_a_member"
    assert backend["writes"] == []


def test_description_of_a_session_in_an_unknown_project_is_a_404(
    member: TestClient, backend: dict
):
    response = member.put(
        description_url(OPEN["name"], project="nope"), json={"description": "x"}
    )
    assert response.status_code == 404
    assert response.json()["code"] == "project_not_found"


def test_description_of_an_unknown_session_is_a_404(member: TestClient, backend: dict):
    response = member.put(description_url("nope"), json={"description": "x"})
    assert response.status_code == 404
    assert response.json()["code"] == "session_not_found"
    assert backend["writes"] == []


@pytest.mark.parametrize(
    ("error", "status", "code"),
    [
        # The file is someone else's.
        (NoAccess("README.md"), 403, "no_access"),
        (NotAPlainFile("README.md"), 400, "description_rejected"),
        # The folder went away after the project was looked up.
        (NotFound("folder"), 404, "session_not_found"),
        (APIError(502, "files_unavailable", "down"), 502, "files_unavailable"),
    ],
)
def test_a_refused_session_write_is_reported(
    member: TestClient, backend: dict, error: Exception, status: int, code: str
):
    backend["write_error"] = error
    response = member.put(description_url(OPEN["name"]), json={"description": "x"})
    assert response.status_code == status
    assert response.json()["code"] == code
    if code != "files_unavailable":
        assert repr(OPEN["name"]) in response.json()["message"]


def test_a_malformed_session_description_is_rejected_before_any_call(
    member: TestClient, backend: dict
):
    response = member.put(
        description_url(OPEN["name"]), json={"description": "a" * 10_001}
    )
    assert response.status_code == 422
    assert backend["writes"] == []


def test_session_description_requires_a_session_cookie(
    settings: Settings, backend: dict
):
    response = api_client(create_app(settings)).put(
        description_url(OPEN["name"]), json={"description": "x"}
    )
    assert response.status_code == 401
    assert backend["writes"] == []


def test_session_description_is_covered_by_the_origin_check(
    member: TestClient, backend: dict
):
    response = member.put(
        description_url(OPEN["name"]),
        json={"description": "x"},
        headers={"Origin": "https://evil.example"},
    )
    assert response.status_code == 403
    assert response.json()["code"] == "bad_origin"
    assert backend["writes"] == []


def test_session_description_reports_an_unreachable_permissions_service(
    member: TestClient, backend: dict, monkeypatch: pytest.MonkeyPatch
):
    def unreachable(self: PermissionsClient, *args, **kwargs):
        raise requests.ConnectionError("permissions service is down")

    monkeypatch.setattr(PermissionsClient, "projectdetails", unreachable)
    response = member.put(description_url(OPEN["name"]), json={"description": "x"})
    assert response.status_code == 502
    assert response.json()["code"] == "permissions_unavailable"
    assert backend["writes"] == []
