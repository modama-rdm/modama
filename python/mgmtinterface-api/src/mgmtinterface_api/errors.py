from typing import Any

from fastapi import Request
from fastapi.responses import JSONResponse
from pydantic import BaseModel, Field


class APIError(Exception):
    """
    The error type for this API. `api_error_handler` turns it into the error
    response body.

    The Flask API answers HTTP 200 with an ``{errno, message}`` envelope. This
    API uses HTTP status codes, so callers can rely on ``response.ok``. `code`
    is a stable, machine-readable string that clients can branch on; `message`
    is human-readable and meant for display.

    The human-readable field is not named ``detail``, because FastAPI's own 422
    validation body uses ``detail`` for a list of per-field errors, and one key
    with two types across the API would be confusing. 422 keeps FastAPI's
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
        # exception is raised, so headers an error response needs (such as
        # `Retry-After`) are attached to the error.
        self.headers = headers


async def api_error_handler(request: Request, exc: Exception) -> JSONResponse:
    # Starlette types handlers as taking a bare Exception; narrow it here rather
    # than silencing the type checker at the registration site.
    assert isinstance(exc, APIError)
    # Error responses must not be cached; some of them (401 in particular)
    # depend on session state.
    headers = {"Cache-Control": "no-store", **(exc.headers or {})}
    return JSONResponse(
        status_code=exc.status_code,
        content={"code": exc.code, "message": exc.message},
        headers=headers,
    )


class ErrorResponse(BaseModel):
    """
    The documented body of every error this API returns, except HTTP 422.

    A model rather than an inline description, so that `error_responses` can
    reference the same schema from every operation and a generated client gets
    one error type instead of one per endpoint.
    """

    code: str = Field(
        description=(
            "Stable, machine-readable reason. Part of the contract; it only "
            "changes with the API version."
        ),
        examples=["not_authenticated"],
    )
    message: str = Field(
        description="Human-readable explanation, safe to show to the user.",
        examples=["Sign in to continue."],
    )


def error_responses(descriptions: dict[int, str]) -> dict[int | str, dict[str, Any]]:
    """
    Build the ``responses=`` argument that documents a set of error statuses.

    Without it the OpenAPI document would list only the success model and
    FastAPI's own 422, and the statuses callers have to handle (400, 401, 403,
    429) would be undocumented. Each layer that raises errors exports its own
    set, so a route lists what it adds rather than repeating what its
    dependencies already document.
    """
    return {
        status: {"model": ErrorResponse, "description": description}
        for status, description in descriptions.items()
    }
