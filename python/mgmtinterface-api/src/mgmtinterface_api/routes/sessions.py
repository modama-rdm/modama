from typing import Annotated

from fastapi import APIRouter, Depends, Path, Request, Response, status
from pydantic import BaseModel, Field, field_validator

from ..errors import APIError, error_responses
from ..permissions import (
    Rejected,
    create_session,
    find_session,
    require_member,
    require_project,
)
from ..security import (
    AUTH_ERROR_RESPONSES,
    CSRF_ERROR_RESPONSES,
    current_session,
    settings_of,
)
from ..session import SessionData
from ..versioning import VERSION_ERROR_RESPONSES

# "Session" is the UI's word for what the permissions service and the legacy
# API call a dataset: one folder per measurement run inside a project, with a
# `raw/` subfolder that sealing makes immutable.
router = APIRouter(
    prefix="/projects/{project}/sessions",
    tags=["sessions"],
    responses={**VERSION_ERROR_RESPONSES},
)

ProjectName = Annotated[str, Path(description="Name of the project.")]


class Session(BaseModel):
    """One session, in the shape the project details of the legacy API use."""

    name: str
    immutable: bool = Field(description="True once the session has been sealed.")
    ctime: float
    mtime: float


class CreateSessionRequest(BaseModel):
    # The same rule the permissions service enforces (`check_valid_dataset`),
    # stated here so that it is in the OpenAPI document. Spaces are allowed;
    # session names are folder names that people type.
    name: str = Field(
        min_length=1,
        max_length=255,
        pattern=r"^[A-Za-z0-9 ._-]+$",
        description=(
            "Up to 255 characters: letters, digits, space, `.`, `_` and `-`. "
            "Becomes the folder name inside the project."
        ),
        examples=["2026-10-16-alignment_run"],
    )

    @field_validator("name")
    @classmethod
    def not_a_directory_reference(cls, name: str) -> str:
        # Both pass the pattern, and `..` would point at the project folder.
        if name in (".", ".."):
            raise ValueError("A session cannot be named '.' or '..'.")
        return name


@router.post(
    "",
    status_code=status.HTTP_201_CREATED,
    summary="Create a session in a project, or return it if it already exists",
    responses={
        200: {
            "model": Session,
            "description": "A session of that name already exists in the project.",
        },
        **AUTH_ERROR_RESPONSES,
        **CSRF_ERROR_RESPONSES,
        **error_responses(
            {
                400: (
                    "The permissions service refused the session. `message` says "
                    "why; it is the error of the `mkdir` run as the signed-in user."
                ),
                403: "The signed-in user is neither owner nor member of the project.",
                404: "There is no project of that name.",
            }
        ),
    },
)
def add_session(
    request: Request,
    response: Response,
    project: ProjectName,
    session: Annotated[SessionData, Depends(current_session)],
    body: CreateSessionRequest,
) -> Session:
    """
    Create a session folder in a project, as the signed-in user.

    Idempotent: a session that already exists is answered with 200 and its
    current state instead of an error, so a client that lost the first
    response can simply retry. A session has no owner of its own; any member
    of the project gets the existing one back.
    """
    settings = settings_of(request)
    username = session["username"]

    details = require_project(settings, project)
    require_member(details, username)

    existing = find_session(details, body.name)
    if existing is not None:
        response.status_code = status.HTTP_200_OK
        return Session(**existing)

    try:
        create_session(settings, project, body.name, username)
    except Rejected as rejected:
        # A concurrent request may have created it between the check above and
        # the `mkdir`; that is the retry this endpoint is meant to absorb, not
        # an error.
        existing = find_session(require_project(settings, project), body.name)
        if existing is None:
            raise APIError(400, "session_rejected", rejected.message) from rejected
        response.status_code = status.HTTP_200_OK
        return Session(**existing)

    # The service answers the create with nothing but `errno`; the timestamps
    # come from reading the project back.
    created = find_session(require_project(settings, project), body.name)
    if created is None:
        raise APIError(
            502,
            "permissions_unavailable",
            "The session was created but could not be read back. Please reload.",
        )
    return Session(**created)
