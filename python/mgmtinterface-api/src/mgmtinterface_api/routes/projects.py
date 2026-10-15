from typing import Annotated

from fastapi import APIRouter, Depends, Request
from pydantic import BaseModel

from ..permissions import list_projects
from ..security import AUTH_ERROR_RESPONSES, current_session, settings_of
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
