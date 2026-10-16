from collections.abc import Iterable

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


def project_details(settings: Settings, name: str) -> ProjectDetailsResponse | None:
    """
    A project with its sessions, or None when there is no ad-hoc project of
    that name.

    Always the fast variant: the full one runs `du` over the whole project to
    fill `usage_bytes`, which none of the callers here need.
    """
    try:
        result = make_client(settings).projectdetails(name, fast=True)
    except requests.RequestException as exc:
        raise _unavailable() from exc
    if result.get("errno") != 0:
        # The only refusal this endpoint has is "not an ad-hoc project".
        return None
    return result


def require_project(settings: Settings, name: str) -> ProjectDetailsResponse:
    """`project_details`, with a 404 instead of None."""
    details = project_details(settings, name)
    if details is None:
        raise APIError(404, "project_not_found", f"There is no project named {name!r}.")
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


def find_session(
    details: ProjectDetailsResponse, name: str
) -> SerializedDataset | None:
    for dataset in details["datasets"]:
        if dataset["name"] == name:
            return dataset
    return None


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
