from typing import Annotated

from fastapi import APIRouter, Depends, Request, Response, status
from pydantic import BaseModel, Field

from ..errors import APIError, error_responses
from ..permissions import Rejected, create_project, list_projects, project_details
from ..security import (
    AUTH_ERROR_RESPONSES,
    CSRF_ERROR_RESPONSES,
    current_session,
    settings_of,
)
from ..session import SessionData
from ..versioning import VERSION_ERROR_RESPONSES

router = APIRouter(
    prefix="/projects", tags=["projects"], responses={**VERSION_ERROR_RESPONSES}
)


class Project(BaseModel):
    """
    One project, in the shape the permissions service returns.

    The fields are identical to those of the legacy `/api/project/` response, so
    that screens can migrate one endpoint at a time without changing how they
    read a project. `usage_bytes` is null in `ProjectList`; it is expensive to
    compute, so the permissions service only computes it for a single project.
    """

    relpath: str
    name: str
    owner: str
    members: list[str]
    ctime: float
    mtime: float
    gid: int
    usage_bytes: int | None


class ProjectList(BaseModel):
    # An object rather than a bare array, so pagination fields can be added
    # later without breaking every caller (Theme A / #90).
    projects: list[Project]


class CreateProjectRequest(BaseModel):
    # The same rule the permissions service enforces (`check_valid_group`),
    # stated here so that it is in the OpenAPI document and a bad name fails
    # before a round trip. The service stays the authority: it also refuses
    # names that collide with a Unix group or an existing folder.
    name: str = Field(
        min_length=3,
        max_length=32,
        pattern=r"^[a-z][a-z0-9_-]*$",
        description=(
            "3 to 32 characters: lowercase letters, digits, `_` and `-`, starting "
            "with a letter. Becomes the folder name, and later possibly the Unix "
            "group name."
        ),
        examples=["ghost_imaging_2026"],
    )
    members: list[Annotated[str, Field(min_length=1)]] = Field(
        default_factory=list,
        description=(
            "Usernames to add as members. The signed-in user becomes the owner "
            "and is a member regardless of this list."
        ),
    )


# Sync `def`: the permissions call is blocking, so FastAPI runs it in a threadpool.
@router.get(
    "",
    summary="List every project",
    responses={**AUTH_ERROR_RESPONSES},
)
def read_projects(
    request: Request,
    session: Annotated[SessionData, Depends(current_session)],
) -> ProjectList:
    """
    All projects, unfiltered, like the legacy endpoint.

    Grouping into own/member/other happens in the client, and access is
    enforced by the file system rather than by hiding rows here. This changes
    with private projects (Theme D), which need a filtered query.
    """
    return ProjectList(
        projects=[Project(**project) for project in list_projects(settings_of(request))]
    )


@router.post(
    "",
    status_code=status.HTTP_201_CREATED,
    summary="Create a project, or return it if it already exists",
    responses={
        200: {
            "model": Project,
            "description": (
                "A project of that name already exists and belongs to you. It is "
                "returned unchanged; the members in the request are not applied."
            ),
        },
        **AUTH_ERROR_RESPONSES,
        **CSRF_ERROR_RESPONSES,
        **error_responses(
            {
                400: (
                    "The permissions service refused the project. `message` says "
                    "why, for example a name that collides with a Unix group, or "
                    "an account that may not create projects."
                ),
                409: "A project of that name exists and belongs to someone else.",
            }
        ),
    },
)
def add_project(
    request: Request,
    response: Response,
    session: Annotated[SessionData, Depends(current_session)],
    body: CreateProjectRequest,
) -> Project:
    """
    Create an ad-hoc project owned by the signed-in user.

    Idempotent: sending the same request again answers 200 with the existing
    project instead of an error, so a client that lost the first response can
    simply retry. Only a project that belongs to someone else is a conflict.
    The existing project is returned as it is; changing its members is a
    separate operation.
    """
    settings = settings_of(request)
    username = session["username"]
    # The owner is a member whatever the list says, and the service adds each
    # name once; drop duplicates and the owner so that it does not have to.
    members = [m for m in dict.fromkeys(body.members) if m != username]

    try:
        created = create_project(settings, body.name, owner=username, members=members)
    except Rejected as rejected:
        # Create first, look second: looking first would leave a window in
        # which two concurrent requests both see nothing and one of them fails
        # with the very error this endpoint is meant to absorb.
        existing = project_details(settings, body.name)
        if existing is None:
            raise APIError(400, "project_rejected", rejected.message) from rejected
        owner = existing["project"]["owner"]
        if owner != username:
            raise APIError(
                409,
                "project_exists",
                f"A project named {body.name!r} already exists and belongs to {owner}.",
            ) from rejected
        response.status_code = status.HTTP_200_OK
        return Project(**existing["project"])

    # The service fills `usage_bytes` on a create (a `du` of an empty folder)
    # but not on the lookup above, and both outcomes of this endpoint should
    # answer with the same body. Null, as in the list, is the one that costs
    # nothing; the project details endpoint is where the size belongs.
    return Project(**{**created, "usage_bytes": None})
