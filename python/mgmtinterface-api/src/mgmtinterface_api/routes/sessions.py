from typing import Annotated

from fastapi import APIRouter, Depends, Path, Request, Response, status
from pydantic import AfterValidator, BaseModel, Field, field_validator

from ..descriptions import (
    DESCRIPTION_MAX_LENGTH,
    change_description,
    describe_new,
    normalize_description,
    session_folder,
)
from ..errors import APIError, error_responses
from ..permissions import (
    Rejected,
    create_session,
    find_session,
    require_member,
    require_project,
    require_session,
    seal_session,
    session_not_found,
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
# API call a dataset: the data in a folder directly inside a project folder,
# with a `raw/` subfolder that sealing makes immutable.
router = APIRouter(
    prefix="/projects/{project}/sessions",
    tags=["sessions"],
    responses={**VERSION_ERROR_RESPONSES},
)

ProjectName = Annotated[str, Path(description="Name of the project.")]
SessionName = Annotated[
    str,
    Path(
        description=(
            "Name of the session, URL-encoded where needed; spaces and dots "
            "are allowed in session names."
        )
    ),
]


# The optional description on the create requests.
Description = Annotated[
    str | None,
    Field(
        max_length=DESCRIPTION_MAX_LENGTH,
        description=(
            "Free text, saved as `README.md` in the new folder. Markdown is "
            "welcome; the UI renders it. Blank is the same as absent: no file."
        ),
        examples=["Off-axis holography of Ni nanowires, Titan G2, Feb 2026."],
    ),
    AfterValidator(normalize_description),
]


class SetDescriptionRequest(BaseModel):
    description: str = Field(
        max_length=DESCRIPTION_MAX_LENGTH,
        description=(
            "The new description, saved as `README.md` in the folder. An empty "
            "text, or one of only whitespace, removes the file."
        ),
        examples=["Off-axis holography of Ni nanowires, Titan G2, Feb 2026."],
    )

    @field_validator("description")
    @classmethod
    def normalized(cls, text: str) -> str:
        return normalize_description(text) or ""


class Session(BaseModel):
    """One session, in the shape the project details of the legacy API use."""

    name: str
    immutable: bool = Field(
        description="True once the session has been sealed: its `raw/` folder is immutable."
    )
    ctime: float
    mtime: float
    description: str | None = Field(
        default=None,
        description=(
            "The text of the session folder's `README.md`, next to `raw/`, or "
            "null when there is none."
        ),
    )


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
    description: Description = None

    @field_validator("name")
    @classmethod
    def not_a_directory_reference(cls, name: str) -> str:
        # Both pass the pattern, and `..` would point at the project folder.
        if name in (".", ".."):
            raise ValueError("A session cannot be named '.' or '..'.")
        # It would collide with the project's own description file.
        if name.lower() == "readme.md":
            raise ValueError("A session cannot be named README.md.")
        return name


@router.post(
    "",
    status_code=status.HTTP_201_CREATED,
    summary="Create a session in a project, or return it if it already exists",
    responses={
        200: {
            "model": Session,
            "description": (
                "A session of that name already exists in the project. It is "
                "returned as it is; the description in the request is not applied."
            ),
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
                502: (
                    "`description_not_saved`: the session was created, but its "
                    "description could not be written."
                ),
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
    of the project gets the existing one back. Its description is returned as
    it is; the one in the request is not applied (see `PUT .../description`).

    A `description` is saved as `README.md` in the session folder, next to
    `raw/`, owned by the signed-in user. It is written over WebDAV once the
    folder exists; if that fails, the session stays and the answer is a 502.
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

    if body.description is not None:
        describe_new(
            settings,
            username,
            session_folder(details["project"]["relpath"], body.name),
            body.description,
            what=f"session {body.name!r}",
        )

    # The service answers the create with nothing but `errno`; the timestamps
    # and the description come from reading the project back.
    created = find_session(require_project(settings, project), body.name)
    if created is None:
        raise APIError(
            502,
            "permissions_unavailable",
            "The session was created but could not be read back. Please reload.",
        )
    return Session(**created)


@router.post(
    "/{session}/seal",
    summary="Seal a session: make its raw data immutable",
    responses={
        **AUTH_ERROR_RESPONSES,
        **CSRF_ERROR_RESPONSES,
        **error_responses(
            {
                400: (
                    "The permissions service refused to seal the session. "
                    "`message` says why, for example a missing `raw/` folder."
                ),
                403: "The signed-in user is neither owner nor member of the project.",
                404: "There is no such project, or no such session in it.",
            }
        ),
    },
)
def seal(
    request: Request,
    project: ProjectName,
    session: SessionName,
    # Named for what it is here, because `session` is the path parameter.
    signed_in: Annotated[SessionData, Depends(current_session)],
) -> Session:
    """
    Make the `raw/` folder of a session and everything in it immutable. This
    cannot be undone through the API.

    Idempotent: a session that is already sealed is answered with 200 and its
    current state, without asking the permissions service again. The rest of
    the session folder stays writable; only `raw/` is sealed.
    """
    settings = settings_of(request)
    username = signed_in["username"]

    details = require_project(settings, project)
    require_member(details, username)
    existing = require_session(details, session)
    if existing["immutable"]:
        return Session(**existing)

    try:
        seal_session(settings, project, session, username)
    except Rejected as rejected:
        raise APIError(400, "seal_rejected", rejected.message) from rejected

    # The service answers with nothing but `errno`; the sealed state and the
    # new mtime come from reading the project back.
    sealed = find_session(require_project(settings, project), session)
    if sealed is None:
        raise APIError(
            502,
            "permissions_unavailable",
            "The session was sealed but could not be read back. Please reload.",
        )
    return Session(**sealed)


@router.put(
    "/{session}/description",
    summary="Set or remove the description of a session",
    responses={
        **AUTH_ERROR_RESPONSES,
        **CSRF_ERROR_RESPONSES,
        **error_responses(
            {
                400: (
                    "`README.md` is there, but not as a plain file (a folder or "
                    "a FIFO), so it is left alone."
                ),
                403: (
                    "The signed-in user is neither owner nor member of the "
                    "project (`not_a_member`), or the file system does not let "
                    "them change `README.md` (`no_access`)."
                ),
                404: "There is no such project, or no such session in it.",
                502: "The file server did not answer.",
            }
        ),
    },
)
def set_description(
    request: Request,
    project: ProjectName,
    session: SessionName,
    signed_in: Annotated[SessionData, Depends(current_session)],
    body: SetDescriptionRequest,
) -> Session:
    """
    Write the session's description to `README.md` in its folder, next to
    `raw/`, or remove the file when the description is empty. Sealing covers
    only `raw/`, so a sealed session can still be described, for example once
    the data is in.

    The file is written over WebDAV as the signed-in user, so the file system
    decides, as it does over SMB: a new file belongs to whoever writes it, and
    only they can change it afterwards. Removing it takes the file's owner or
    the owner of the session folder. Sending the same text again changes
    nothing.
    """
    settings = settings_of(request)
    username = signed_in["username"]

    details = require_project(settings, project)
    require_member(details, username)
    require_session(details, session)

    change_description(
        settings,
        username,
        session_folder(details["project"]["relpath"], session),
        body.description or None,
        what=f"session {session!r}",
        missing=session_not_found(project, session),
    )

    # The session, with the description as the permissions service reads it,
    # comes from reading the project back.
    updated = find_session(require_project(settings, project), session)
    if updated is None:
        raise APIError(
            502,
            "permissions_unavailable",
            "The description was saved but could not be read back. Please reload.",
        )
    return Session(**updated)
