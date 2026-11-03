from collections.abc import Iterable
from typing import Literal

import requests
from permissions_client import PermissionsClient
from permissions_client.schema import (
    ProjectDetailsResponse,
    SerializedDataset,
    SerializedProject,
)

from .config import Settings
from .errors import APIError


class Rejected(Exception):
    """
    The permissions service refused a request on its own grounds.

    It answers HTTP 200 with ``{"errno": n, "message": ...}`` for anything from
    an invalid name to a failed `mkdir`, without a code to tell them apart. The
    message is written for the person who made the request, so routes pass it
    on; which status it gets depends on the route, because some of these
    refusals mean "already there" and are not errors for an idempotent create.
    """

    def __init__(self, message: str):
        super().__init__(message)
        self.message = message


def make_client(settings: Settings) -> PermissionsClient:
    """
    A new client per call.

    `PermissionsClient.authenticate()` stores the verified credentials on the
    instance, so a shared client could leak one request's identity into a
    concurrent request. The client has no connection pool of its own (it uses
    the module-level `requests` functions), so creating one is cheap.
    """
    return PermissionsClient(settings.permissions_api)


def authenticate(settings: Settings, username: str, password: str) -> bool:
    try:
        result = make_client(settings).authenticate(username, password)
    except requests.RequestException as exc:
        # Unlike `user_exists`, there is no fallback here: without the
        # permissions service a password cannot be checked, and a 401 would tell
        # the user that the credentials are wrong when they may be correct.
        raise _unavailable() from exc
    return result.get("errno") == 0


def list_projects(settings: Settings) -> list[SerializedProject]:
    try:
        result = make_client(settings).list_projects()
    except requests.RequestException as exc:
        raise _unavailable() from exc
    if result.get("errno") != 0:
        # The permissions service reported an error. Nothing here can act on
        # it, and its message is aimed at operators, so answer with our own.
        raise _unavailable()
    return result["projects"]


def list_users(settings: Settings) -> list[str]:
    """Everyone who may use the system: the members of the ad-hoc group."""
    try:
        result = make_client(settings).list_users()
    except requests.RequestException as exc:
        raise _unavailable() from exc
    if result.get("errno") != 0:
        raise _unavailable()
    return sorted(result["users"])


def project_details(settings: Settings, name: str) -> ProjectDetailsResponse | None:
    """
    A project with its sessions, or None when there is no ad-hoc project of
    that name.

    Always the fast variant: the full one runs `du` over the whole project to
    fill `usage_bytes`, which only `project_usage` asks for.
    """
    try:
        result = make_client(settings).projectdetails(name, fast=True)
    except requests.RequestException as exc:
        raise _unavailable() from exc
    if result.get("errno") != 0:
        # The only refusal this endpoint has is "not an ad-hoc project".
        return None
    return result


def project_usage(settings: Settings, name: str) -> int | None:
    """
    The disk usage of a project in bytes, or None when there is no ad-hoc
    project of that name.

    This is the one call that asks for the slow variant, which runs `du` over
    the project folder. It has its own endpoint so that nothing else waits on
    it.
    """
    try:
        result = make_client(settings).projectdetails(name, fast=False)
    except requests.RequestException as exc:
        raise _unavailable() from exc
    if result.get("errno") != 0:
        return None
    usage = result["project"]["usage_bytes"]
    if usage is None:
        # The slow variant always fills it; a null here means the service did
        # not do what was asked.
        raise _unavailable()
    return int(usage)


def project_not_found(name: str) -> APIError:
    return APIError(404, "project_not_found", f"There is no project named {name!r}.")


def require_project(settings: Settings, name: str) -> ProjectDetailsResponse:
    """`project_details`, with a 404 instead of None."""
    details = project_details(settings, name)
    if details is None:
        raise project_not_found(name)
    return details


def require_member(details: ProjectDetailsResponse, username: str) -> None:
    """
    Only the owner and the members of a project may add to it. The file
    system enforces this too (the project folder grants others read-only
    access); the check here turns the resulting `mkdir` failure into a 403
    that says why.
    """
    project = details["project"]
    if username != project["owner"] and username not in project["members"]:
        raise APIError(
            403,
            "not_a_member",
            f"You are not a member of project {project['name']!r}.",
        )


def require_owner(details: ProjectDetailsResponse, username: str) -> None:
    """
    Only the owner may change who the members are. The permissions service
    checks this too (`check_can_modify`); the check here turns its refusal
    into a 403 that says why.
    """
    project = details["project"]
    if username != project["owner"]:
        raise APIError(
            403,
            "not_the_owner",
            f"Only the owner of project {project['name']!r}, {project['owner']}, "
            "can change its members.",
        )


def find_session(
    details: ProjectDetailsResponse, name: str
) -> SerializedDataset | None:
    for dataset in details["datasets"]:
        if dataset["name"] == name:
            return dataset
    return None


def session_not_found(project: str, name: str) -> APIError:
    return APIError(
        404,
        "session_not_found",
        f"There is no session named {name!r} in project {project!r}.",
    )


def require_session(details: ProjectDetailsResponse, name: str) -> SerializedDataset:
    """`find_session`, with a 404 instead of None."""
    session = find_session(details, name)
    if session is None:
        raise session_not_found(details["project"]["name"], name)
    return session


def create_project(
    settings: Settings, name: str, owner: str, members: Iterable[str]
) -> SerializedProject:
    try:
        result = make_client(settings).create_project(name, owner, members)
    except requests.RequestException as exc:
        raise _unavailable() from exc
    if result.get("errno") != 0:
        raise Rejected(str(result.get("message", "The project was not created.")))
    return result["project"]


def create_session(settings: Settings, project: str, name: str, user: str) -> None:
    """
    Create a session folder (a dataset, in the permissions service's terms) as
    `user`. The service does the `mkdir` as that Unix user, so the file system
    enforces membership; a refusal comes back as a message from `mkdir`.
    """
    try:
        result = make_client(settings).create_dataset(project, name, user)
    except requests.RequestException as exc:
        raise _unavailable() from exc
    if result.get("errno") != 0:
        raise Rejected(str(result.get("message", "The session was not created.")))


def seal_session(settings: Settings, project: str, name: str, user: str) -> None:
    """
    Make the `raw/` folder of a session and everything in it immutable, as
    `user`. The service checks that `user` is the owner or a member, that the
    folder exists and that it is not a symlink; any refusal comes back as a
    message.
    """
    try:
        result = make_client(settings).seal_dataset(project, name, user)
    except requests.RequestException as exc:
        raise _unavailable() from exc
    if result.get("errno") != 0:
        raise Rejected(str(result.get("message", "The session was not sealed.")))


def add_members(
    settings: Settings, project: str, members: Iterable[str], user: str
) -> SerializedProject:
    """
    Grant `members` access to a project, as `user`. Returns the project as the
    service serializes it afterwards.
    """
    return _change_members(settings, project, "add", members, user)


def remove_members(
    settings: Settings, project: str, members: Iterable[str], user: str
) -> SerializedProject:
    """Revoke the access of `members`, as `user`. See `add_members`."""
    return _change_members(settings, project, "remove", members, user)


def _change_members(
    settings: Settings,
    project: str,
    op: Literal["add", "remove"],
    members: Iterable[str],
    user: str,
) -> SerializedProject:
    try:
        result = make_client(settings).change_project_members(
            project, op, list(members), user
        )
    except requests.RequestException as exc:
        raise _unavailable() from exc
    if result.get("errno") != 0:
        raise Rejected(str(result.get("message", "The members were not changed.")))
    return result["project"]


def user_uid(settings: Settings, username: str) -> int:
    try:
        result = make_client(settings).id_user(username)
    except requests.RequestException as exc:
        raise _unavailable() from exc
    if result.get("errno") != 0:
        raise _unavailable()
    return int(result["id"]["uid"])


def _unavailable() -> APIError:
    return APIError(
        502,
        "permissions_unavailable",
        "The account service is unavailable. Please try again shortly.",
    )


def user_exists(settings: Settings, username: str) -> bool:
    """
    Check without a password that an account still exists. Used when refreshing
    a session, because the cookie carries no credentials to re-authenticate
    with.

    This only confirms that the account resolves, not that it is still enabled
    or that its password is unchanged. The short idle lifetime limits that gap.
    """
    try:
        result = make_client(settings).id_user(username)
    except requests.RequestException:
        # An unreachable permissions service means the account cannot be
        # checked, not that it is invalid. Failing closed would log every user
        # out during a restart of that service. The session is validly signed,
        # so it stays and is re-checked at the next refresh.
        return True
    return result.get("errno") == 0
