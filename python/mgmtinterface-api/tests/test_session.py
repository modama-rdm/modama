import time
from typing import Any

import pytest
from conftest import issue_backdated
from itsdangerous import URLSafeTimedSerializer
from mgmtinterface_api.session import _SALT, SessionCodec, SessionInvalid

SECRET = "test-secret"
MAX_AGE = 8 * 60 * 60
IDLE = 1800
PERSISTENT_MAX_AGE = 7 * 24 * 60 * 60


@pytest.fixture
def codec() -> SessionCodec:
    return SessionCodec(
        SECRET,
        max_age_seconds=MAX_AGE,
        idle_seconds=IDLE,
        persistent_max_age_seconds=PERSISTENT_MAX_AGE,
    )


def legacy_token(payload: dict[str, Any]) -> str:
    """
    A cookie as an older deployment would have signed it: same secret and salt,
    whatever payload that code wrote.
    """
    return URLSafeTimedSerializer(SECRET, salt=_SALT).dumps(payload)


def test_roundtrip(codec: SessionCodec):
    data, age = codec.read(codec.issue("mmustermann"))
    assert data["username"] == "mmustermann"
    assert data["persistent"] is False
    assert age < 1


def test_persistent_roundtrip(codec: SessionCodec):
    data, _ = codec.read(codec.issue("mmustermann", persistent=True))
    assert data["persistent"] is True


def test_tampered_cookie_is_rejected(codec: SessionCodec):
    # Alter the payload, not the trailing signature byte: the signature covers
    # the payload text exactly, so this always breaks it, whereas two different
    # final base64url characters can decode to the same bytes.
    payload, _, rest = codec.issue("mmustermann").partition(".")
    swapped = payload[:-1] + ("A" if payload[-1] != "A" else "B")
    with pytest.raises(SessionInvalid):
        _ = codec.read(f"{swapped}.{rest}")


def test_cookie_from_another_secret_is_rejected(codec: SessionCodec):
    other = SessionCodec(
        "other-secret",
        max_age_seconds=MAX_AGE,
        idle_seconds=IDLE,
        persistent_max_age_seconds=PERSISTENT_MAX_AGE,
    )
    with pytest.raises(SessionInvalid):
        _ = codec.read(other.issue("mmustermann"))


def test_idle_timeout(codec: SessionCodec, monkeypatch: pytest.MonkeyPatch):
    token = issue_backdated(codec, monkeypatch, IDLE + 1, username="mmustermann")
    with pytest.raises(SessionInvalid):
        _ = codec.read(token)


def test_persistent_session_ignores_idle_timeout(
    codec: SessionCodec, monkeypatch: pytest.MonkeyPatch
):
    """The point of the flag: a cookie idle for far longer than `IDLE` still reads."""
    token = issue_backdated(
        codec, monkeypatch, IDLE + 3600, username="mmustermann", persistent=True
    )
    data, age = codec.read(token)
    assert data["persistent"] is True
    # The age is still reported, so the caller refreshes it as usual.
    assert age >= IDLE + 3600


def test_persistent_session_has_its_own_absolute_limit():
    codec = SessionCodec(
        SECRET, max_age_seconds=1, idle_seconds=IDLE, persistent_max_age_seconds=10
    )
    now = int(time.time())

    with pytest.raises(SessionInvalid):
        _ = codec.read(codec.issue("mmustermann", issued=now - 11, persistent=True))

    # Within the persistent limit, although far past the normal one.
    data, _ = codec.read(codec.issue("mmustermann", issued=now - 5, persistent=True))
    assert data["issued"] == now - 5

    # The normal limit still applies to a normal session.
    with pytest.raises(SessionInvalid):
        _ = codec.read(codec.issue("mmustermann", issued=now - 5))


def test_cookie_without_persistent_key_reads_as_not_persistent(codec: SessionCodec):
    # Cookies issued before the flag existed stay valid, as normal sessions.
    token = legacy_token({"username": "mmustermann", "issued": int(time.time())})
    data, _ = codec.read(token)
    assert data["persistent"] is False


def test_non_bool_persistent_is_rejected(codec: SessionCodec):
    token = legacy_token(
        {"username": "mmustermann", "issued": int(time.time()), "persistent": "yes"}
    )
    with pytest.raises(SessionInvalid):
        _ = codec.read(token)


def test_absolute_lifetime_survives_refresh(codec: SessionCodec):
    """
    `issued` is carried separately so that refreshing moves the idle window but
    does not extend the absolute lifetime.
    """
    issued_long_ago = int(time.time()) - 9 * 60 * 60
    refreshed_now = codec.issue("mmustermann", issued=issued_long_ago)
    with pytest.raises(SessionInvalid):
        _ = codec.read(refreshed_now)


def test_refresh_keeps_the_session_alive_within_the_absolute_window(
    codec: SessionCodec,
):
    issued_recently = int(time.time()) - 60
    data, _ = codec.read(codec.issue("mmustermann", issued=issued_recently))
    assert data["issued"] == issued_recently
