from fastapi import Request
from fastapi.responses import JSONResponse


class APIError(Exception):
    """
    The one error type for this API. Raise it anywhere; `api_error_handler`
    turns it into the single response shape the API promises.

    The legacy Flask API answers HTTP 200 with an ``{errno, message}`` envelope.
    This API uses real status codes instead, so callers can rely on
    ``response.ok``. `code` is a stable machine-readable string (safe to branch
    on), `message` is human-readable and safe to display.

    The human-readable field is deliberately *not* named ``detail``: FastAPI's
    own 422 validation body uses ``detail`` for a *list* of per-field errors,
    and one key with two types across the API is a trap. 422 keeps FastAPI's
    default shape and is the documented exception to this convention.
    """

    def __init__(
        self,
        status_code: int,
        code: str,
        message: str,
        headers: dict[str, str] | None = None,
    ):
        super().__init__(message)
        self.status_code = status_code
        self.code = code
        self.message = message
        # Headers a handler sets on its injected `Response` are lost once an
        # exception takes over, so anything an error must carry (`Retry-After`,
        # say) has to travel with the error itself.
        self.headers = headers


async def api_error_handler(request: Request, exc: Exception) -> JSONResponse:
    # Starlette types handlers as taking a bare Exception; narrow it here rather
    # than silencing the type checker at the registration site.
    assert isinstance(exc, APIError)
    # Error responses are never cacheable, and some of them (401s especially)
    # reflect session state.
    headers = {"Cache-Control": "no-store", **(exc.headers or {})}
    return JSONResponse(
        status_code=exc.status_code,
        content={"code": exc.code, "message": exc.message},
        headers=headers,
    )
