import time

import pytest
from itsdangerous.timed import TimestampSigner
from mgmtinterface_api.session import SessionCodec, SessionInvalid


@pytest.fixture
def codec() -> SessionCodec:
    return SessionCodec("test-secret", max_age_seconds=8 * 60 * 60, idle_seconds=1800)


def test_roundtrip(codec: SessionCodec):
    data, age = codec.read(codec.issue("mmustermann"))
    assert data["username"] == "mmustermann"
    assert age < 1


def test_tampered_cookie_is_rejected(codec: SessionCodec):
    # Alter the payload, not the trailing signature byte: the signature covers
    # the payload text exactly, so this always breaks it, whereas two different
    # final base64url characters can decode to the same bytes.
    payload, _, rest = codec.issue("mmustermann").partition(".")
    swapped = payload[:-1] + ("A" if payload[-1] != "A" else "B")
    with pytest.raises(SessionInvalid):
        _ = codec.read(f"{swapped}.{rest}")


def test_cookie_from_another_secret_is_rejected(codec: SessionCodec):
    other = SessionCodec("other-secret", max_age_seconds=28800, idle_seconds=1800)
    with pytest.raises(SessionInvalid):
        _ = codec.read(other.issue("mmustermann"))


def test_idle_timeout(codec: SessionCodec, monkeypatch: pytest.MonkeyPatch):
    # itsdangerous stamps tokens with whole seconds, so backdate the stamp
    # instead of sleeping the test out.
    real_timestamp = TimestampSigner.get_timestamp
    monkeypatch.setattr(
        TimestampSigner, "get_timestamp", lambda self: real_timestamp(self) - 3600
    )
    token = codec.issue("mmustermann")
    monkeypatch.undo()

    with pytest.raises(SessionInvalid):
        _ = codec.read(token)


def test_absolute_lifetime_survives_refresh(codec: SessionCodec):
    """
    The whole point of carrying `issued` separately: refreshing moves the idle
    window but must not extend the absolute one.
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
