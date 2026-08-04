import requests
from permissions_client import PermissionsClient
from permissions_client.schema import SerializedProject

from .config import Settings
from .errors import APIError


def make_client(settings: Settings) -> PermissionsClient:
    """
    A fresh client per use.

    `PermissionsClient.authenticate()` stores the credentials it verified on the
    instance, so a shared client would leak one request's identity into another
    request handled concurrently. The client keeps no connection pool of its own
    (it calls `requests` module-level), so a new one costs nothing.
    """
    return PermissionsClient(settings.permissions_api)


def authenticate(settings: Settings, username: str, password: str) -> bool:
    try:
        result = make_client(settings).authenticate(username, password)
    except requests.RequestException as exc:
        # Unlike `user_exists`, there is nothing to fall back on here: without
        # the permissions service we cannot tell a good password from a bad one,
        # and answering 401 would tell the user their credentials are wrong when
        # they are not.
        raise _unavailable() from exc
    return result.get("errno") == 0


def list_projects(settings: Settings) -> list[SerializedProject]:
    try:
        result = make_client(settings).list_projects()
    except requests.RequestException as exc:
        raise _unavailable() from exc
    if result.get("errno") != 0:
        # The upstream reported a logical failure. Nothing here can act on it,
        # and its wording is aimed at operators, so answer with our own.
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
    Password-free check that an account is still known, used when refreshing a
    session — the cookie carries no credentials to re-authenticate with.

    Note what this does *not* catch: it confirms the account resolves, not that
    it is still enabled or that its password has not been changed. A short idle
    lifetime is what bounds that gap.
    """
    try:
        result = make_client(settings).id_user(username)
    except requests.RequestException:
        # An unreachable permissions service means "cannot confirm", not
        # "invalid". Failing closed would sign every user out during a restart
        # of that service; the session is already validly signed, so let it
        # stand and re-check at the next refresh.
        return True
    return result.get("errno") == 0
