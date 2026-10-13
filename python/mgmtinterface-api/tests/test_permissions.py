import pytest
import requests
from mgmtinterface_api.config import Settings
from mgmtinterface_api.errors import APIError
from mgmtinterface_api.permissions import authenticate, user_exists
from permissions_client import PermissionsClient


def unreachable(*args: object, **kwargs: object):
    raise requests.ConnectionError("permissions service is down")


def test_login_reports_an_unreachable_permissions_service(
    settings: Settings, monkeypatch: pytest.MonkeyPatch
):
    """
    Not a 401: telling users that their password is wrong when the account
    service is down would make them reset credentials that are correct.
    """
    monkeypatch.setattr(PermissionsClient, "authenticate", unreachable)
    with pytest.raises(APIError) as raised:
        _ = authenticate(settings, "mmustermann", "correct horse")
    assert raised.value.status_code == 502
    assert raised.value.code == "permissions_unavailable"


def test_refresh_keeps_the_session_when_permissions_is_unreachable(
    settings: Settings, monkeypatch: pytest.MonkeyPatch
):
    """
    The opposite decision: this runs against an existing signed session, so
    failing closed would log everyone out during a restart of that service.
    """
    monkeypatch.setattr(PermissionsClient, "id_user", unreachable)
    assert user_exists(settings, "mmustermann") is True
