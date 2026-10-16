from typing import Annotated

from fastapi import APIRouter, Depends, Request
from pydantic import BaseModel, Field

from ..permissions import list_users
from ..security import AUTH_ERROR_RESPONSES, current_session, settings_of
from ..session import SessionData
from ..versioning import VERSION_ERROR_RESPONSES

router = APIRouter(
    prefix="/users", tags=["users"], responses={**VERSION_ERROR_RESPONSES}
)


class UserList(BaseModel):
    # An object rather than a bare array, like `ProjectList`, so that fields
    # can be added without breaking callers.
    users: list[str] = Field(
        description=(
            "Usernames of everyone who may use the data management system, "
            "sorted. These are the accounts a project can have as members."
        )
    )


# Sync `def`: the permissions call is blocking, so FastAPI runs it in a threadpool.
@router.get(
    "",
    summary="List every user",
    responses={**AUTH_ERROR_RESPONSES},
)
def read_users(
    request: Request,
    session: Annotated[SessionData, Depends(current_session)],
) -> UserList:
    """
    Every account that may use the system, for picking the members of a
    project. Unfiltered, like the legacy endpoint; narrowing happens in the
    client.
    """
    return UserList(users=list_users(settings_of(request)))
