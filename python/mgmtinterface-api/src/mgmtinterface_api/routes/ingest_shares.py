from typing import Annotated

from fastapi import APIRouter, Depends, Header, Path, Request, Response, status
from pydantic import BaseModel, Field
from smbdrop_client.schema import SerializedShare

from ..agent_tokens import verified_resource
from ..config import Settings
from ..errors import APIError, error_responses
from ..permissions import require_member, require_project, require_session, user_uid
from ..security import (
    AUTH_ERROR_RESPONSES,
    CSRF_ERROR_RESPONSES,
    current_session,
    settings_of,
)
from ..session import SessionData
from ..shares import (
    create_share,
    delete_share,
    find_share,
    list_shares,
    share_path,
    split_share_path,
    user_shares,
)
from ..versioning import VERSION_ERROR_RESPONSES

# "Ingest share" is the UI's word for an smbdrop share: a temporary Samba share
# of one session with one-time credentials, which an instrument computer maps to
# write data straight into the session as the user who opened it.
router = APIRouter(
    prefix="/ingest-shares",
    tags=["ingest shares"],
    responses={**VERSION_ERROR_RESPONSES},
)


class IngestShare(BaseModel):
    """
    An ingest share. The fields smbdrop reports, plus the project and session
    it belongs to, so that a client does not parse `dest_path`, and the host
    and workgroup the web interface shows a person who connects by hand.
    modama-agent does not use those two: an instrument computer may reach the Samba
    server another way than the intranet does, so the agent mounts with the
    host, port and domain from its own configuration file.
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
        description="The resource the share was opened for, if an agent opened it."
    )
    created_by: str | None = Field(
        default=None,
        description=(
            "Login name of the user who opened the share. Null for shares "
            "opened before this was recorded."
        ),
    )
    smb_host: str = Field(
        description=(
            "Samba host as seen from the intranet, for a person connecting by "
            "hand. modama-agent uses its own configured host instead."
        )
    )
    workgroup: str = Field(
        description="Workgroup for the username on Windows, for connecting by hand."
    )


class IngestShareList(BaseModel):
    # An object rather than a bare array, like `ProjectList`, and a list rather
    # than the legacy map keyed by share id: each share names its project and
    # session, so a client no longer has to parse `dest_path` to find the one
    # it wants.
    shares: list[IngestShare] = Field(
        description="The signed-in user's open shares, oldest first."
    )


class CreateIngestShareRequest(BaseModel):
    project: str = Field(min_length=1, examples=["ghost_imaging_2026"])
    session: str = Field(min_length=1, examples=["2026-10-16-alignment_run"])
    resource: str | None = Field(
        default=None,
        min_length=1,
        description=(
            "The resource to bind the share to. modama-agent sends it "
            "together with `X-Agent-Token`, which must have been issued for "
            "this resource; other callers leave both out."
        ),
    )


ShareId = Annotated[str, Path(description="The `share_id` of an open share.")]

AgentToken = Annotated[
    str | None,
    Header(
        alias="X-Agent-Token",
        description=(
            "Token from smbdrop's token service, which modama-agent fetches with "
            "its client certificate. It says which resource the request comes "
            "from; the session still says who the user is. Sent together with "
            "`resource` in the body, or not at all."
        ),
    ),
]


def _ingest_share(
    settings: Settings, share: SerializedShare, project: str, session: str
) -> IngestShare:
    """An smbdrop share plus what this API adds to it."""
    return IngestShare(
        **share,
        project=project,
        session=session,
        smb_host=settings.ingest_smb_host,
        workgroup=settings.ingest_workgroup,
    )


@router.get(
    "",
    summary="List your open ingest shares",
    responses={
        **AUTH_ERROR_RESPONSES,
        **error_responses(
            {502: "The ingest share service or the account service is unavailable."}
        ),
    },
)
def read_ingest_shares(
    request: Request,
    session: Annotated[SessionData, Depends(current_session)],
) -> IngestShareList:
    """
    The shares the signed-in user has open, across all projects. Shares write
    as the user who opened them, so another user's shares are not listed, as
    in the legacy endpoint.
    """
    settings = settings_of(request)
    uid = user_uid(settings, session["username"])
    return IngestShareList(
        shares=[
            _ingest_share(settings, share, *split_share_path(share["dest_path"]))
            for share in user_shares(settings, uid)
        ]
    )


@router.delete(
    "/{share_id}",
    status_code=status.HTTP_204_NO_CONTENT,
    summary="Close an ingest share",
    responses={
        **AUTH_ERROR_RESPONSES,
        **CSRF_ERROR_RESPONSES,
        **error_responses(
            {
                403: "The share was opened by someone else.",
                404: (
                    "There is no open ingest share with that id. An ingest "
                    "share that was closed already answers this too."
                ),
                502: "The ingest share service or the account service is unavailable.",
            }
        ),
    },
)
def close_ingest_share(
    request: Request,
    share_id: ShareId,
    session: Annotated[SessionData, Depends(current_session)],
) -> None:
    """
    Close a share: its one-time credentials stop working and the Samba export
    goes away. The data written through it stays in the session.

    Only the user who opened a share can close it. A share that is gone
    already is a 404; a client that lost the answer to its first attempt can
    treat that as closed.
    """
    settings = settings_of(request)
    share = list_shares(settings).get(share_id)
    if share is None:
        raise APIError(
            404,
            "share_not_found",
            f"There is no open ingest share {share_id!r}. It may have been closed.",
        )
    if share["uid"] != user_uid(settings, session["username"]):
        raise APIError(
            403, "not_your_share", "This ingest share was opened by someone else."
        )
    delete_share(settings, share_id)


@router.post(
    "",
    status_code=status.HTTP_201_CREATED,
    summary="Open an ingest share for a session, or return the one already open",
    responses={
        200: {
            "model": IngestShare,
            "description": "You already have an ingest share open for this session.",
        },
        **AUTH_ERROR_RESPONSES,
        **CSRF_ERROR_RESPONSES,
        **error_responses(
            {
                400: "Only one of `X-Agent-Token` and `resource` was sent.",
                403: (
                    "The signed-in user is neither owner nor member of the "
                    "project, or the agent token is invalid, expired, or for "
                    "another resource."
                ),
                404: "There is no such project, or no such session in it.",
                502: (
                    "The ingest share service, the account service or the agent token "
                    "service is unavailable."
                ),
            }
        ),
    },
)
def add_ingest_share(
    request: Request,
    response: Response,
    session: Annotated[SessionData, Depends(current_session)],
    body: CreateIngestShareRequest,
    agent_token: AgentToken = None,
) -> IngestShare:
    """
    Open a share for a session that writes as the signed-in user.

    Idempotent per user and session: if the signed-in user already has a share
    for that session, it is answered with 200 instead of a second share, so a
    client that lost the first response can simply retry. Another user's share
    for the same session is not reused; each user ingests under their own
    identity.

    modama-agent also sends `X-Agent-Token` and `resource`, which bind the share
    to the instrument it runs on, so that smbdrop offers the share to that
    instrument's agent. The token must have been issued for that `resource`. A
    repeat then counts only a share bound to the same instrument.
    """
    settings = settings_of(request)
    username = session["username"]

    if (agent_token is None) != (body.resource is None):
        raise APIError(
            400,
            "agent_binding_incomplete",
            "Send X-Agent-Token and resource together, or neither.",
        )

    details = require_project(settings, body.project)
    require_member(details, username)
    require_session(details, body.session)

    resource = None
    if agent_token is not None:
        resource = verified_resource(settings, agent_token)
        if resource != body.resource:
            raise APIError(
                403,
                "resource_mismatch",
                f"The agent token was issued for {resource!r}, not {body.resource!r}.",
            )

    project = details["project"]
    uid = user_uid(settings, username)
    dest_path = share_path(settings, project["relpath"], body.session)

    existing = find_share(settings, dest_path, uid, resource)
    if existing is not None:
        response.status_code = status.HTTP_200_OK
        share = existing
    else:
        share = create_share(
            settings,
            dest_path,
            uid=uid,
            gid=project["gid"],
            resource=resource,
            created_by=username,
        )

    return _ingest_share(settings, share, project["name"], body.session)
