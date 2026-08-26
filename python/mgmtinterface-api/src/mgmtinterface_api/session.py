import time
from typing import Any, TypedDict, cast

from fastapi import Response
from itsdangerous import BadSignature, URLSafeTimedSerializer

from .config import Settings

# Namespaces the signature, so a token minted for something else can never be
# read back as a session even under the same secret.
_SALT = "modama-session"


class SessionData(TypedDict):
    username: str
    # Unix time of the *original* login. The serializer carries its own
    # timestamp, which moves forward on every refresh; this one never does, so
    # it is what bounds the absolute lifetime.
    issued: int


class SessionInvalid(Exception):
    """Cookie missing, tampered with, malformed, or past either lifetime."""


class SessionCodec:
    """
    Signs and verifies the session cookie.

    Stateless by decision (#172): the cookie *is* the session, so there is no
    store to consult and nothing to revoke. Logout deletes the client's copy,
    but a leaked cookie stays valid until it expires — which is why the idle
    lifetime is short. Revisit when private projects (Theme D) need real
    session-kill.
    """

    def __init__(self, secret: str, max_age_seconds: int, idle_seconds: int):
        self._serializer = URLSafeTimedSerializer(secret, salt=_SALT)
        self._max_age_seconds = max_age_seconds
        self._idle_seconds = idle_seconds

    def issue(self, username: str, issued: int | None = None) -> str:
        """
        Mint a cookie value. Pass the original `issued` when refreshing, so a
        sliding session cannot outlive its absolute lifetime.
        """
        data: SessionData = {
            "username": username,
            "issued": int(time.time()) if issued is None else issued,
        }
        return self._serializer.dumps(data)

    def read(self, token: str) -> tuple[SessionData, float]:
        """
        Verify a cookie value, returning the session and how many seconds have
        passed since it was last issued (the caller uses that to decide whether
        to refresh).
        """
        try:
            raw, refreshed_at = self._serializer.loads(
                token, max_age=self._idle_seconds, return_timestamp=True
            )
        except BadSignature as exc:
            # Also covers SignatureExpired, i.e. the idle timeout.
            raise SessionInvalid("bad or idle-expired session cookie") from exc

        data = _as_session_data(raw)
        now = time.time()
        if now - data["issued"] > self._max_age_seconds:
            raise SessionInvalid("session is past its absolute lifetime")
        return data, now - refreshed_at.timestamp()


def _as_session_data(raw: Any) -> SessionData:
    # The signature proves we minted it, not that the payload still matches this
    # version of the code — an older deployment's shape must not crash a loader.
    if (
        not isinstance(raw, dict)
        or not isinstance(raw.get("username"), str)
        or not isinstance(raw.get("issued"), int)
    ):
        raise SessionInvalid("session cookie payload has an unexpected shape")
    return cast(SessionData, raw)


# `__Host-` requires exactly this path and no Domain attribute. It is also the
# right path for after the cutover (#170), when the SPA moves to the root.
_COOKIE_PATH = "/"


def set_session_cookie(response: Response, settings: Settings, token: str) -> None:
    response.set_cookie(
        settings.effective_cookie_name,
        token,
        max_age=settings.session_max_age_seconds,
        httponly=True,
        secure=settings.session_cookie_secure,
        # Lax, not Strict: Strict would drop the cookie when a user follows a
        # link into the app from an ELN entry, which is a normal way in here.
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
    The same deletion, rendered as a header mapping for `APIError`.

    A dead session is refused by *raising*, and headers written to a handler's
    injected `Response` are dropped once an exception handler builds its own
    response — so the deletion has to travel with the error instead. Rendering
    it through a throwaway `Response` keeps one definition of the cookie's
    attributes; they must match the ones it was set with or the browser keeps
    the original cookie alongside the deletion.
    """
    carrier = Response()
    clear_session_cookie(carrier, settings)
    return {"set-cookie": carrier.headers["set-cookie"]}
