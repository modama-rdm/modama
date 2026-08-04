from typing import Annotated

from fastapi import APIRouter, Depends, Request, Response, status
from pydantic import BaseModel, Field

from ..errors import APIError
from ..permissions import authenticate
from ..security import codec_of, current_session, settings_of, throttle_of
from ..session import SessionData, clear_session_cookie, set_session_cookie

# The CSRF origin check is applied app-wide in `create_app`, so a new router
# cannot forget it.
router = APIRouter(prefix="/auth", tags=["auth"])


class LoginRequest(BaseModel):
    username: str = Field(min_length=1)
    password: str = Field(min_length=1)


class SessionInfo(BaseModel):
    username: str


def _no_store(response: Response) -> None:
    # Responses that carry or establish session state must not be cached by a
    # proxy or the browser's back/forward cache.
    response.headers["Cache-Control"] = "no-store"


# Sync `def`: authenticating calls the permissions service over blocking HTTP,
# so FastAPI should run this in a threadpool rather than on the event loop.
@router.post("/login")
def login(request: Request, response: Response, body: LoginRequest) -> SessionInfo:
    settings = settings_of(request)
    _no_store(response)

    # Usernames are case-insensitive to the directory, so fold the throttle key
    # or alternating case would multiply an attacker's budget.
    throttle_key = body.username.strip().casefold()
    throttle = throttle_of(request)
    if throttle.is_blocked(throttle_key):
        raise APIError(
            429,
            "too_many_attempts",
            "Too many failed sign-in attempts. Please wait and try again.",
            headers={"Retry-After": str(throttle.retry_after_seconds(throttle_key))},
        )

    if not authenticate(settings, body.username, body.password):
        throttle.record_failure(throttle_key)
        # One message for both "no such user" and "wrong password", so the
        # response does not confirm which usernames exist. Deliberately no
        # WWW-Authenticate header: that is what made the browser show its own
        # login dialog on top of the SPA's.
        raise APIError(401, "invalid_credentials", "Wrong username or password.")

    throttle.reset(throttle_key)
    set_session_cookie(response, settings, codec_of(request).issue(body.username))
    return SessionInfo(username=body.username)


@router.post("/logout", status_code=status.HTTP_204_NO_CONTENT)
def logout(request: Request, response: Response) -> None:
    # No session required: logging out of an already-expired session should
    # succeed quietly rather than 401.
    _no_store(response)
    clear_session_cookie(response, settings_of(request))


@router.get("/session")
def read_session(
    response: Response,
    session: Annotated[SessionData, Depends(current_session)],
) -> SessionInfo:
    """Who the caller is; 401 when there is no usable session."""
    _no_store(response)
    return SessionInfo(username=session["username"])
