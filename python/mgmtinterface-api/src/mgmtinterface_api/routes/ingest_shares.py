from typing import Annotated

from fastapi import APIRouter, Depends, Request, Response, status
from pydantic import BaseModel, Field

from ..errors import APIError, error_responses
from ..permissions import find_session, require_member, require_project, user_uid
from ..security import (
    AUTH_ERROR_RESPONSES,
    CSRF_ERROR_RESPONSES,
    current_session,
    settings_of,
)
from ..session import SessionData
from ..shares import create_share, find_share, share_path
from ..versioning import VERSION_ERROR_RESPONSES

# "Ingest share" is the UI's word for an smbdrop share: a temporary Samba share
# of one session with one-time credentials, which an instrument PC maps to
# write data straight into the session as the user who opened it.
router = APIRouter(
    prefix="/ingest-shares",
    tags=["ingest shares"],
    responses={**VERSION_ERROR_RESPONSES},
)


class IngestShare(BaseModel):
    """
    An active share. The fields smbdrop reports, plus the project and session
    it belongs to and the host to connect to, so that a client needs neither to
    parse `dest_path` nor to know the lab's Samba host.
    """

    share_id: str
    share_name: str = Field(description="The share part of the SMB address.")
    project: str
    session: str
    dest_path: str = Field(description="The exported folder, as smbdrop sees it.")
    created_timestamp: int = Field(description="Unix time of creation.")
    username: str = Field(description="One-time Samba username.")
    password: str = Field(description="One-time Samba password.")
    uid: int = Field(description="Unix uid the share writes as.")
    gid: int = Field(description="Unix gid the share writes as.")
    resource: str | None = Field(
        description="The instrument the share was opened for, if an agent opened it."
    )
    smb_host: str = Field(description="Samba host to connect to.")
    workgroup: str = Field(description="Workgroup for the username on Windows.")


class CreateIngestShareRequest(BaseModel):
    project: str = Field(min_length=1, examples=["ghost_imaging_2026"])
    session: str = Field(min_length=1, examples=["2026-10-16-alignment_run"])


@router.post(
    "",
    status_code=status.HTTP_201_CREATED,
    summary="Open an ingest share for a session, or return the one already open",
    responses={
        200: {
            "model": IngestShare,
            "description": "You already have a share open for this session.",
        },
        **AUTH_ERROR_RESPONSES,
        **CSRF_ERROR_RESPONSES,
        **error_responses(
            {
                403: "The signed-in user is neither owner nor member of the project.",
                404: "There is no such project, or no such session in it.",
                502: "The share service or the account service is unavailable.",
            }
        ),
    },
)
def add_ingest_share(
    request: Request,
    response: Response,
    session: Annotated[SessionData, Depends(current_session)],
    body: CreateIngestShareRequest,
) -> IngestShare:
    """
    Open a share for a session that writes as the signed-in user.

    Idempotent per user and session: if the signed-in user already has a share
    for that session, it is answered with 200 instead of a second share, so a
    client that lost the first response can simply retry. Another user's share
    for the same session is not reused; each user ingests under their own
    identity.
    """
    settings = settings_of(request)
    username = session["username"]

    details = require_project(settings, body.project)
    require_member(details, username)
    if find_session(details, body.session) is None:
        raise APIError(
            404,
            "session_not_found",
            f"There is no session named {body.session!r} in project {body.project!r}.",
        )

    project = details["project"]
    uid = user_uid(settings, username)
    dest_path = share_path(settings, project["relpath"], body.session)

    existing = find_share(settings, dest_path, uid)
    if existing is not None:
        response.status_code = status.HTTP_200_OK
        share = existing
    else:
        share = create_share(settings, dest_path, uid=uid, gid=project["gid"])

    return IngestShare(
        **share,
        project=project["name"],
        session=body.session,
        smb_host=settings.ingest_smb_host,
        workgroup=settings.ingest_workgroup,
    )
