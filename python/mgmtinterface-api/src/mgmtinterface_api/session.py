import time
from typing import Any, NamedTuple, TypedDict

from fastapi import Response
from itsdangerous import BadSignature, URLSafeTimedSerializer

from .config import Settings

# Namespaces the signature, so a token signed for another purpose with the same
# secret cannot be read back as a session.
_SALT = "modama-session"


class SessionData(TypedDict):
    username: str
    # Unix time (UTC) of the original login. The serializer carries its own
    # timestamp, which moves forward on every refresh; this one never does, so
    # it is what bounds the absolute lifetime.
    issued: int
    # Opened with `persistent: true` ("keep me logged in"): no idle timeout and
    # a longer absolute lifetime. Cookies issued before the flag existed lack
    # the key; `_as_session_data` reads them as not persistent.
    persistent: bool


class VerifiedSession(NamedTuple):
    """A session read back from a cookie, and how stale that cookie is."""

    data: SessionData
    age_seconds: float


class SessionInvalid(Exception):
    """Cookie missing, tampered with, malformed, or past either lifetime."""


class SessionCodec:
    """
    Signs and verifies the session cookie.

    Sessions are implemented using signed cookies, so there is no store to
    consult and nothing to revoke. Logout deletes the client's copy, but a
    leaked cookie stays valid until it expires, which is why the idle lifetime
    is short. Revisit when private projects (Theme D) need sessions that an
    admin can end.

    Two separate timestamps bound a session:

    * `itsdangerous` stamps the token itself and that stamp moves forward every
      time the cookie is re-issued. `read()` checks it against `idle_seconds`,
      so it measures time since the last refresh: the idle window.
    * `issued` is carried in the payload and copied unchanged across refreshes.
      It measures time since the original login and bounds the absolute
      lifetime (`max_age_seconds`), which a sliding session must not extend.

    A persistent session ("keep me logged in") skips the idle check and is
    bounded by `persistent_max_age_seconds` instead of `max_age_seconds`. The
    flag travels in the signed payload, so each cookie carries the lifetimes
    that apply to it and nothing has to be stored server-side.

    Refreshing happens in `security.current_session`, not here; this class only
    reports how stale a cookie is, as the age returned by `read()`. The caller
    re-issues at most every `session_refresh_seconds`, which is also where the
    account is re-checked against the permissions service. Login and logout are
    in `routes/auth.py`.
    """

    def __init__(
        self,
        secret: str,
        max_age_seconds: int,
        idle_seconds: int,
        persistent_max_age_seconds: int,
    ):
        self._serializer = URLSafeTimedSerializer(secret, salt=_SALT)
        self._max_age_seconds = max_age_seconds
        self._idle_seconds = idle_seconds
        self._persistent_max_age_seconds = persistent_max_age_seconds

    def issue(
        self, username: str, issued: int | None = None, persistent: bool = False
    ) -> str:
        """
        Create a cookie value. Pass the original `issued` and `persistent` when
        refreshing, so that a refreshed session keeps its kind and cannot
        outlive its absolute lifetime.
        """
        data: SessionData = {
            "username": username,
            "issued": int(time.time()) if issued is None else issued,
            "persistent": persistent,
        }
        return self._serializer.dumps(data)

    def read(self, token: str) -> VerifiedSession:
        """
        Verify a cookie value, returning the session and how many seconds have
        passed since it was last issued (the caller uses that to decide whether
        to refresh).
        """
        try:
            raw, refreshed_at = self._serializer.loads(token, return_timestamp=True)
        except BadSignature as exc:
            raise SessionInvalid("bad session cookie") from exc

        data = _as_session_data(raw)
        now = time.time()
        age = now - refreshed_at.timestamp()
        # The idle check is done here rather than with `loads(max_age=...)`:
        # whether it applies depends on the flag inside the payload, which is
        # only known once `loads` has returned.
        if data["persistent"]:
            limit = self._persistent_max_age_seconds
        else:
            if age > self._idle_seconds:
                raise SessionInvalid("session is past its idle lifetime")
            limit = self._max_age_seconds
        if now - data["issued"] > limit:
            raise SessionInvalid("session is past its absolute lifetime")
        return VerifiedSession(data, age)


def _as_session_data(raw: Any) -> SessionData:
    # The signature proves that this service issued the token, not that the
    # payload matches the current code. A payload written by an older deployment
    # must not crash the loader.
    if (
        not isinstance(raw, dict)
        or not isinstance(raw.get("username"), str)
        or not isinstance(raw.get("issued"), int)
        # Absent in cookies issued before the flag existed; a present one must
        # be a real boolean.
        or not isinstance(raw.get("persistent", False), bool)
    ):
        raise SessionInvalid("session cookie payload has an unexpected shape")
    return {
        "username": raw["username"],
        "issued": raw["issued"],
        "persistent": raw.get("persistent", False),
    }


# `__Host-` requires exactly this path and no Domain attribute. It is also the
# right path for after the cutover (#170), when the SPA moves to the root.
_COOKIE_PATH = "/"


def set_session_cookie(
    response: Response, settings: Settings, token: str, persistent: bool = False
) -> None:
    # `Max-Age` follows the kind of session: a persistent cookie has to survive
    # browser restarts for its whole lifetime.
    max_age = (
        settings.session_persistent_max_age_seconds
        if persistent
        else settings.session_max_age_seconds
    )
    response.set_cookie(
        settings.effective_cookie_name,
        token,
        max_age=max_age,
        httponly=True,
        secure=settings.session_cookie_secure,
        # Lax rather than Strict: Strict would drop the cookie when a user
        # follows a link into the app from an ELN entry, which is a common way
        # to get here.
        samesite="lax",
        path=_COOKIE_PATH,
    )


def clear_session_cookie(response: Response, settings: Settings) -> None:
    # Attributes have to match the ones used to set it or the browser keeps the
    # original cookie alongside the deletion.
    response.delete_cookie(
        settings.effective_cookie_name,
        httponly=True,
        secure=settings.session_cookie_secure,
        samesite="lax",
        path=_COOKIE_PATH,
    )


def clear_session_cookie_header(settings: Settings) -> dict[str, str]:
    """
    The same deletion as a header mapping, for `APIError`.

    An expired session is rejected by raising, and headers written to the
    handler's injected `Response` are dropped once the exception handler builds
    its own response, so the deletion has to be attached to the error. Using a
    throwaway `Response` keeps a single definition of the cookie attributes;
    they must match the ones the cookie was set with, or the browser keeps the
    original cookie next to the deletion.
    """
    carrier = Response()
    clear_session_cookie(carrier, settings)
    return {"set-cookie": carrier.headers["set-cookie"]}
