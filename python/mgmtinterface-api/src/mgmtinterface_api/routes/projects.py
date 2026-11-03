from typing import Annotated

from fastapi import APIRouter, Depends, Request, Response, status
from pydantic import BaseModel, Field

from ..errors import APIError, error_responses
from ..permissions import (
    Rejected,
    add_members,
    create_project,
    list_projects,
    project_details,
    project_not_found,
    project_usage,
    remove_members,
    require_member,
    require_owner,
    require_project,
    set_project_description,
)
from ..security import (
    AUTH_ERROR_RESPONSES,
    CSRF_ERROR_RESPONSES,
    current_session,
    settings_of,
)
from ..session import SessionData
from ..versioning import VERSION_ERROR_RESPONSES
from .sessions import Description, ProjectName, Session, SetDescriptionRequest

router = APIRouter(
    prefix="/projects", tags=["projects"], responses={**VERSION_ERROR_RESPONSES}
)


class Project(BaseModel):
    """
    One project, in the shape the permissions service returns.

    The fields are identical to those of the legacy `/api/project/` response, so
    that screens can migrate one endpoint at a time without changing how they
    read a project. `usage_bytes` is null everywhere but in the answer of
    `GET /projects/{project}/usage`: computing it means a `du` over the whole
    project, which nothing else should wait for.
    """

    relpath: str
    name: str
    owner: str
    members: list[str]
    ctime: float
    mtime: float
    gid: int
    usage_bytes: int | None
    description: str | None = Field(
        default=None,
        description=(
            "The text of the project folder's `README.md`, or null when there is none."
        ),
    )


class ProjectList(BaseModel):
    # An object rather than a bare array, so pagination fields can be added
    # later without breaking every caller (Theme A / #90).
    projects: list[Project]


class ProjectDetails(Project):
    """A project with its sessions. `usage_bytes` is null; see `GET .../usage`."""

    sessions: list[Session]


class ProjectUsage(BaseModel):
    usage_bytes: int = Field(
        description="Disk usage of the project folder in bytes, as `du` reports it."
    )


class SetMembersRequest(BaseModel):
    members: list[Annotated[str, Field(min_length=1)]] = Field(
        description=(
            "The complete list of members wanted. Members not in it are removed, "
            "names not yet members are added. The owner is a member regardless "
            "and may be left out."
        ),
        examples=[["clausen", "weber"]],
    )


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
    description: Description = None


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
                "returned unchanged; the members and the description in the "
                "request are not applied."
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
    The existing project is returned as it is; changing its members or its
    description is a separate operation.

    A `description` is saved as `README.md` in the project folder, owned by
    the signed-in user.
    """
    settings = settings_of(request)
    username = session["username"]
    # The owner is a member whatever the list says, and the service adds each
    # name once; drop duplicates and the owner so that it does not have to.
    members = [m for m in dict.fromkeys(body.members) if m != username]

    try:
        created = create_project(
            settings,
            body.name,
            owner=username,
            members=members,
            description=body.description,
        )
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
    # nothing; `GET /projects/{project}/usage` is where the size belongs.
    return Project(**{**created, "usage_bytes": None})


@router.get(
    "/{project}",
    summary="A project with its sessions",
    responses={
        **AUTH_ERROR_RESPONSES,
        **error_responses({404: "There is no project of that name."}),
    },
)
def read_project(
    request: Request,
    project: ProjectName,
    session: Annotated[SessionData, Depends(current_session)],
) -> ProjectDetails:
    """
    One project and the sessions in it, readable by anyone signed in, like the
    list. Access to the data is enforced by the file system, not by hiding the
    project here.

    `usage_bytes` is null: the size needs a `du` over the project, which can
    take a minute on a large one, so it has its own endpoint.
    """
    details = require_project(settings_of(request), project)
    return ProjectDetails(
        **details["project"],
        sessions=[Session(**dataset) for dataset in details["datasets"]],
    )


@router.get(
    "/{project}/usage",
    summary="The disk usage of a project",
    responses={
        **AUTH_ERROR_RESPONSES,
        **error_responses({404: "There is no project of that name."}),
    },
)
def read_project_usage(
    request: Request,
    project: ProjectName,
    session: Annotated[SessionData, Depends(current_session)],
) -> ProjectUsage:
    """
    The size of the project folder. Slow: the permissions service runs `du`
    over the whole project, so call this after the details, not instead.
    """
    usage = project_usage(settings_of(request), project)
    if usage is None:
        raise project_not_found(project)
    return ProjectUsage(usage_bytes=usage)


@router.put(
    "/{project}/members",
    summary="Set the members of a project",
    responses={
        **AUTH_ERROR_RESPONSES,
        **CSRF_ERROR_RESPONSES,
        **error_responses(
            {
                400: (
                    "The permissions service refused the change; `message` says "
                    "why. `members_rejected` means nothing was changed. "
                    "`members_partially_changed` means the new members were "
                    "added but the dropped ones could not be removed; the "
                    "message names both. Sending the request again applies "
                    "what is still missing."
                ),
                403: "Only the owner of a project can change its members.",
                404: "There is no project of that name.",
            }
        ),
    },
)
def set_members(
    request: Request,
    project: ProjectName,
    session: Annotated[SessionData, Depends(current_session)],
    body: SetMembersRequest,
) -> Project:
    """
    Replace the members of a project with the given list.

    The API works out who to add and who to remove against the current
    members, so the client sends the list it wants rather than operations.
    Sending the same list again changes nothing and answers with the project
    as it is, which also makes a retry after a lost or partial answer safe:
    the difference is computed afresh each time. The owner is a member
    whatever the list says and is never added or removed.
    """
    settings = settings_of(request)
    username = session["username"]

    details = require_project(settings, project)
    require_owner(details, username)
    current = details["project"]
    owner = current["owner"]

    wanted = {member for member in body.members if member != owner}
    have = {member for member in current["members"] if member != owner}
    to_add = sorted(wanted - have)
    to_remove = sorted(have - wanted)
    if not to_add and not to_remove:
        return Project(**current)

    # The service applies one operation per call, so add and remove cannot be
    # atomic. Add first: a failure then leaves the project as it was, and the
    # error can say so. The service answers each call with the project after
    # that call, which saves reading it back.
    updated = current
    if to_add:
        try:
            updated = add_members(settings, project, to_add, username)
        except Rejected as rejected:
            raise APIError(400, "members_rejected", rejected.message) from rejected
    if to_remove:
        try:
            updated = remove_members(settings, project, to_remove, username)
        except Rejected as rejected:
            if to_add:
                raise APIError(
                    400,
                    "members_partially_changed",
                    f"Added {', '.join(to_add)}, but could not remove "
                    f"{', '.join(to_remove)}: {rejected.message}",
                ) from rejected
            raise APIError(400, "members_rejected", rejected.message) from rejected

    # The service serializes the project with a `du` after a change; the size
    # is not this endpoint's to report. Null, as everywhere but `GET .../usage`.
    return Project(**{**updated, "usage_bytes": None})


@router.put(
    "/{project}/description",
    summary="Set or remove the description of a project",
    responses={
        **AUTH_ERROR_RESPONSES,
        **CSRF_ERROR_RESPONSES,
        **error_responses(
            {
                400: (
                    "The permissions service refused the change. `message` says "
                    "why, for example a `README.md` that is not a regular file."
                ),
                403: "The signed-in user is neither owner nor member of the project.",
                404: "There is no project of that name.",
            }
        ),
    },
)
def set_description(
    request: Request,
    project: ProjectName,
    session: Annotated[SessionData, Depends(current_session)],
    body: SetDescriptionRequest,
) -> Project:
    """
    Write the project's description to `README.md` in the project folder, or
    remove the file when the description is empty.

    An existing file is rewritten in place and keeps its owner; a new one
    belongs to the project's owner. The owner and the members may change it,
    like they may seal sessions. Sending the same text again changes nothing.
    """
    settings = settings_of(request)
    username = session["username"]

    details = require_project(settings, project)
    require_member(details, username)

    try:
        updated = set_project_description(settings, project, body.description, username)
    except Rejected as rejected:
        raise APIError(400, "description_rejected", rejected.message) from rejected

    # Null, as everywhere but `GET .../usage`.
    return Project(**{**updated, "usage_bytes": None})
