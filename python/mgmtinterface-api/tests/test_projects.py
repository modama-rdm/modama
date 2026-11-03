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
    # No README.md in the folder.
    "description": None,
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

# The sessions of `FIXTURE`, as the service lists them (it calls them datasets).
SESSIONS = [
    {
        "name": "create by hand",
        "immutable": False,
        "ctime": 1.0,
        "mtime": 2.0,
        "description": None,
    },
    {
        "name": "demo session",
        "immutable": True,
        "ctime": 3.0,
        "mtime": 4.0,
        "description": "# Demo\nFirst light on the new detector.",
    },
]


@pytest.fixture
def backend(monkeypatch: pytest.MonkeyPatch) -> dict:
    """
    A permissions service with one existing project (`FIXTURE`, owned by
    user1). Creating any other name succeeds; creating the existing name is
    refused the way the real service refuses it.

    README.md writes, which go over WebDAV, are recorded in `writes` as
    `(user, folder, text)`, text None for a removal, and fail with
    `write_error` when it is set. A write to the fixture's folder shows in the
    project read back, as the permissions service would read the file.
    """
    state: dict = {
        "create_calls": [],
        "reject": None,
        "writes": [],
        "write_error": None,
        "description": None,
    }

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
        assert fast, "only the usage endpoint may ask for the slow variant"
        if projectname != FIXTURE["name"]:
            return {"errno": 1, "message": f'"{projectname}" is not an ad-hoc project.'}
        return {
            "errno": 0,
            "project": {**FIXTURE, "description": state["description"]},
            "datasets": [dict(s) for s in SESSIONS],
        }

    def write_description(
        settings: Settings, username: str, folder: str, text: str | None
    ):
        state["writes"].append((username, folder, text))
        if state["write_error"] is not None:
            raise state["write_error"]
        if folder == "/" + FIXTURE["relpath"]:
            state["description"] = text

    monkeypatch.setattr(PermissionsClient, "create_project", create_project)
    monkeypatch.setattr(PermissionsClient, "projectdetails", projectdetails)
    monkeypatch.setattr(
        "mgmtinterface_api.descriptions.write_description", write_description
    )
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


# --- Descriptions on create ----------------------------------------------------


def test_a_description_is_written_normalized_as_the_owner(
    settings: Settings, backend: dict
):
    client = signed_in_client(create_app(settings), settings, "user1")
    response = client.post(
        PROJECTS,
        json={"name": NEW_NAME, "description": "  Ni nanowires\r\n\r\n- Feb 2026  \n"},
    )
    assert response.status_code == 201
    # LF line ends and no surrounding whitespace, into the new folder, after
    # the permissions service created it.
    assert backend["writes"] == [
        ("user1", f"/adhoc/{NEW_NAME}", "Ni nanowires\n\n- Feb 2026")
    ]
    assert response.json()["description"] == "Ni nanowires\n\n- Feb 2026"


@pytest.mark.parametrize("blank", [None, "", "   ", "\r\n\n\t"])
def test_without_a_description_no_file_is_written(
    settings: Settings, backend: dict, blank: str | None
):
    client = signed_in_client(create_app(settings), settings, "user1")
    body = (
        {"name": NEW_NAME}
        if blank is None
        else {"name": NEW_NAME, "description": blank}
    )
    response = client.post(PROJECTS, json=body)
    assert response.status_code == 201
    assert backend["writes"] == []
    assert response.json()["description"] is None


def test_a_description_of_ten_thousand_characters_is_accepted(
    settings: Settings, backend: dict
):
    client = signed_in_client(create_app(settings), settings, "user1")
    text = "ä" * 10_000
    response = client.post(PROJECTS, json={"name": NEW_NAME, "description": text})
    assert response.status_code == 201
    assert backend["writes"] == [("user1", f"/adhoc/{NEW_NAME}", text)]


def test_a_longer_description_is_rejected_before_any_call(
    settings: Settings, backend: dict
):
    client = signed_in_client(create_app(settings), settings, "user1")
    response = client.post(
        PROJECTS, json={"name": NEW_NAME, "description": "a" * 10_001}
    )
    assert response.status_code == 422
    assert backend["create_calls"] == []
    assert backend["writes"] == []


def test_repeating_a_create_does_not_apply_the_description(
    settings: Settings, backend: dict
):
    # Like the members: the existing project comes back as it is. Changing
    # its description is `PUT .../description`.
    client = signed_in_client(create_app(settings), settings, FIXTURE["owner"])
    response = client.post(
        PROJECTS, json={"name": FIXTURE["name"], "description": "something else"}
    )
    assert response.status_code == 200
    assert response.json()["description"] is None
    assert backend["writes"] == []


@pytest.mark.parametrize(
    "error",
    [
        NoAccess("/adhoc/x/README.md"),
        NotAPlainFile("/adhoc/x/README.md"),
        NotFound("/adhoc/x"),
        APIError(502, "files_unavailable", "The file server is unavailable."),
    ],
)
def test_a_description_that_cannot_be_written_leaves_the_project(
    settings: Settings, backend: dict, error: Exception
):
    backend["write_error"] = error
    client = signed_in_client(create_app(settings), settings, "user1")
    response = client.post(PROJECTS, json={"name": NEW_NAME, "description": "x"})
    assert response.status_code == 502
    assert response.json()["code"] == "description_not_saved"
    assert NEW_NAME in response.json()["message"]
    # Created, and not taken back: a retry gets it with a 200.
    assert backend["create_calls"] == [(NEW_NAME, "user1", [])]


def test_a_service_without_descriptions_reads_as_none(
    settings: Settings, monkeypatch: pytest.MonkeyPatch
):
    # A permissions service that predates descriptions sends no `description`.
    old = {key: value for key, value in FIXTURE.items() if key != "description"}
    monkeypatch.setattr(
        PermissionsClient, "list_projects", lambda self: {"errno": 0, "projects": [old]}
    )
    client = signed_in_client(create_app(settings), settings, "user1")
    response = client.get(PROJECTS)
    assert response.status_code == 200
    assert response.json()["projects"][0]["description"] is None


# --- GET /projects/{project} --------------------------------------------------

DETAILS = f"{PROJECTS}/{FIXTURE['name']}"


def test_reads_a_project_with_its_sessions(settings: Settings, backend: dict):
    client = signed_in_client(create_app(settings), settings, "user1")
    response = client.get(DETAILS)
    assert response.status_code == 200
    # The project's own fields, flat, plus the sessions under the UI's name
    # for them. No `errno` envelope and no nesting under `project`.
    assert response.json() == {**FIXTURE, "sessions": SESSIONS}


def test_details_are_readable_by_anyone_signed_in(settings: Settings, backend: dict):
    # Like the list and the legacy endpoint: access to the data is enforced by
    # the file system, not by hiding the project. user3 is not a member.
    outsider = signed_in_client(create_app(settings), settings, "user3")
    assert outsider.get(DETAILS).status_code == 200


def test_details_do_not_compute_the_size(settings: Settings, backend: dict):
    # The fixture asserts that only the fast variant is asked for; the size is
    # the usage endpoint's job.
    client = signed_in_client(create_app(settings), settings, "user1")
    assert client.get(DETAILS).json()["usage_bytes"] is None


def test_unknown_project_is_a_404(settings: Settings, backend: dict):
    client = signed_in_client(create_app(settings), settings, "user1")
    response = client.get(f"{PROJECTS}/nope")
    assert response.status_code == 404
    assert response.json()["code"] == "project_not_found"


def test_details_require_a_session(settings: Settings, backend: dict):
    assert api_client(create_app(settings)).get(DETAILS).status_code == 401


def test_details_report_an_unreachable_permissions_service(
    settings: Settings, monkeypatch: pytest.MonkeyPatch
):
    def unreachable(self: PermissionsClient, *args, **kwargs):
        raise requests.ConnectionError("permissions service is down")

    monkeypatch.setattr(PermissionsClient, "projectdetails", unreachable)
    client = signed_in_client(create_app(settings), settings, "user1")
    response = client.get(DETAILS)
    assert response.status_code == 502
    assert response.json()["code"] == "permissions_unavailable"


# --- GET /projects/{project}/usage --------------------------------------------

USAGE = f"{DETAILS}/usage"
SIZE = 2_226_346_428_528


@pytest.fixture
def usage_backend(monkeypatch: pytest.MonkeyPatch) -> list[bool]:
    """A permissions service that records which variant was asked for."""
    asked: list[bool] = []

    def projectdetails(self: PermissionsClient, projectname: str, fast: bool = False):
        asked.append(fast)
        if projectname != FIXTURE["name"]:
            return {"errno": 1, "message": f'"{projectname}" is not an ad-hoc project.'}
        project = {**FIXTURE, "usage_bytes": None if fast else SIZE}
        return {"errno": 0, "project": project, "datasets": []}

    monkeypatch.setattr(PermissionsClient, "projectdetails", projectdetails)
    return asked


def test_usage_asks_for_the_slow_variant(settings: Settings, usage_backend: list[bool]):
    client = signed_in_client(create_app(settings), settings, "user1")
    response = client.get(USAGE)
    assert response.status_code == 200
    assert response.json() == {"usage_bytes": SIZE}
    # This is the one call that may run `du`.
    assert usage_backend == [False]


def test_usage_of_an_unknown_project_is_a_404(
    settings: Settings, usage_backend: list[bool]
):
    client = signed_in_client(create_app(settings), settings, "user1")
    response = client.get(f"{PROJECTS}/nope/usage")
    assert response.status_code == 404
    assert response.json()["code"] == "project_not_found"


def test_usage_requires_a_session(settings: Settings, usage_backend: list[bool]):
    assert api_client(create_app(settings)).get(USAGE).status_code == 401
    assert usage_backend == []


def test_a_null_size_from_the_slow_variant_is_a_service_failure(
    settings: Settings, monkeypatch: pytest.MonkeyPatch
):
    # The slow variant always fills the size; null means the service did not
    # do what was asked, and the client must not be told the project is empty.
    monkeypatch.setattr(
        PermissionsClient,
        "projectdetails",
        lambda self, projectname, fast=False: {
            "errno": 0,
            "project": FIXTURE,
            "datasets": [],
        },
    )
    client = signed_in_client(create_app(settings), settings, "user1")
    response = client.get(USAGE)
    assert response.status_code == 502
    assert response.json()["code"] == "permissions_unavailable"


def test_usage_reports_an_unreachable_permissions_service(
    settings: Settings, monkeypatch: pytest.MonkeyPatch
):
    def unreachable(self: PermissionsClient, *args, **kwargs):
        raise requests.ConnectionError("permissions service is down")

    monkeypatch.setattr(PermissionsClient, "projectdetails", unreachable)
    client = signed_in_client(create_app(settings), settings, "user1")
    assert client.get(USAGE).status_code == 502


# --- PUT /projects/{project}/description --------------------------------------

DESCRIPTION = f"{DETAILS}/description"


@pytest.mark.parametrize("user", [FIXTURE["owner"], "user2"])
def test_owner_and_members_may_try_to_set_the_description(
    settings: Settings, backend: dict, user: str
):
    # Whether the file system lets them is the WebDAV server's answer.
    client = signed_in_client(create_app(settings), settings, user)
    response = client.put(DESCRIPTION, json={"description": "  Ni nanowires\r\n"})
    assert response.status_code == 200
    # Read back from the permissions service.
    assert response.json()["description"] == "Ni nanowires"
    assert response.json()["name"] == FIXTURE["name"]
    assert backend["writes"] == [(user, "/adhoc/datamanagement", "Ni nanowires")]


@pytest.mark.parametrize("blank", ["", "  \r\n "])
def test_an_empty_description_removes_it(settings: Settings, backend: dict, blank: str):
    backend["description"] = "old"
    client = signed_in_client(create_app(settings), settings, "user1")
    response = client.put(DESCRIPTION, json={"description": blank})
    assert response.status_code == 200
    assert response.json()["description"] is None
    assert backend["writes"] == [("user1", "/adhoc/datamanagement", None)]


def test_setting_the_description_does_not_report_a_size(
    settings: Settings, backend: dict
):
    client = signed_in_client(create_app(settings), settings, "user1")
    assert (
        client.put(DESCRIPTION, json={"description": "x"}).json()["usage_bytes"] is None
    )


def test_others_may_not_set_the_description(settings: Settings, backend: dict):
    outsider = signed_in_client(create_app(settings), settings, "user3")
    response = outsider.put(DESCRIPTION, json={"description": "mine now"})
    assert response.status_code == 403
    assert response.json()["code"] == "not_a_member"
    assert backend["writes"] == []


def test_description_of_an_unknown_project_is_a_404(settings: Settings, backend: dict):
    client = signed_in_client(create_app(settings), settings, "user1")
    response = client.put(f"{PROJECTS}/nope/description", json={"description": "x"})
    assert response.status_code == 404
    assert response.json()["code"] == "project_not_found"
    assert backend["writes"] == []


@pytest.mark.parametrize(
    ("error", "status", "code"),
    [
        # The file is someone else's.
        (NoAccess("/adhoc/datamanagement/README.md"), 403, "no_access"),
        (NotAPlainFile("/adhoc/datamanagement/README.md"), 400, "description_rejected"),
        # The folder went away after the project was looked up.
        (NotFound("/adhoc/datamanagement"), 404, "project_not_found"),
        (APIError(502, "files_unavailable", "down"), 502, "files_unavailable"),
    ],
)
def test_a_refused_write_is_reported(
    settings: Settings, backend: dict, error: Exception, status: int, code: str
):
    backend["write_error"] = error
    client = signed_in_client(create_app(settings), settings, "user2")
    response = client.put(DESCRIPTION, json={"description": "x"})
    assert response.status_code == status
    assert response.json()["code"] == code
    if code != "files_unavailable":
        assert "'datamanagement'" in response.json()["message"]


@pytest.mark.parametrize(
    "body", [{}, {"description": None}, {"description": "a" * 10_001}]
)
def test_a_malformed_description_is_rejected_before_any_call(
    settings: Settings, backend: dict, body: dict
):
    client = signed_in_client(create_app(settings), settings, "user1")
    assert client.put(DESCRIPTION, json=body).status_code == 422
    assert backend["writes"] == []


def test_setting_the_description_requires_a_session(settings: Settings, backend: dict):
    response = api_client(create_app(settings)).put(
        DESCRIPTION, json={"description": "x"}
    )
    assert response.status_code == 401
    assert backend["writes"] == []


def test_setting_the_description_is_covered_by_the_origin_check(
    settings: Settings, backend: dict
):
    client = signed_in_client(create_app(settings), settings, "user1")
    response = client.put(
        DESCRIPTION,
        json={"description": "x"},
        headers={"Origin": "https://evil.example"},
    )
    assert response.status_code == 403
    assert response.json()["code"] == "bad_origin"
    assert backend["writes"] == []


def test_setting_the_description_reports_an_unreachable_permissions_service(
    settings: Settings, backend: dict, monkeypatch: pytest.MonkeyPatch
):
    def unreachable(self: PermissionsClient, *args, **kwargs):
        raise requests.ConnectionError("permissions service is down")

    monkeypatch.setattr(PermissionsClient, "projectdetails", unreachable)
    client = signed_in_client(create_app(settings), settings, "user1")
    response = client.put(DESCRIPTION, json={"description": "x"})
    assert response.status_code == 502
    assert response.json()["code"] == "permissions_unavailable"
    assert backend["writes"] == []
