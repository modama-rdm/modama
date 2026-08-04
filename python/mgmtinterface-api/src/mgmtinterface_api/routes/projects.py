from typing import Annotated

from fastapi import APIRouter, Depends, Request
from pydantic import BaseModel

from ..permissions import list_projects
from ..security import current_session, settings_of
from ..session import SessionData

router = APIRouter(prefix="/projects", tags=["projects"])


class Project(BaseModel):
    """
    One project, mirroring the permissions service's own shape.

    Kept field-for-field identical to what the legacy `/api/project/` returned,
    so screens migrate one endpoint at a time without also rewriting how they
    read a project. `usage_bytes` is null in the list — it is expensive enough
    that the permissions service only computes it for a single project.
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


# Sync `def`: the permissions call is blocking, so let FastAPI thread it.
@router.get("")
def read_projects(
    request: Request,
    session: Annotated[SessionData, Depends(current_session)],
) -> ProjectList:
    """
    Every project, unfiltered — same as the legacy endpoint.

    Grouping into own/member/other happens client-side, and *access* is enforced
    by the file system rather than by hiding rows here. Private projects (Theme
    D) are what change that, and they need this to become a filtered query.
    """
    return ProjectList(
        projects=[Project(**project) for project in list_projects(settings_of(request))]
    )
