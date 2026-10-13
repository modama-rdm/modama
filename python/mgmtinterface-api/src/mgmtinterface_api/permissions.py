import requests
from permissions_client import PermissionsClient
from permissions_client.schema import SerializedProject

from .config import Settings
from .errors import APIError


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
