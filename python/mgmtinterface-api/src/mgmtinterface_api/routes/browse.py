import base64
import json
from bisect import bisect_right
from typing import Annotated

from fastapi import APIRouter, Depends, Query, Request
from pydantic import BaseModel, Field

from ..errors import APIError, error_responses
from ..links import entry_access, location_access
from ..security import AUTH_ERROR_RESPONSES, current_session, settings_of
from ..session import SessionData
from ..versioning import VERSION_ERROR_RESPONSES
from ..webdav import NoAccess, NotFound, Resource, list_path, normalize_path

router = APIRouter(
    prefix="/browse", tags=["browse"], responses={**VERSION_ERROR_RESPONSES}
)

DEFAULT_LIMIT = 200
# A cap on one answer, not on the folder: every entry is reachable through the
# cursor. It keeps a response, and the table a client renders from it, bounded.
MAX_LIMIT = 1000


class EntryAccess(BaseModel):
    """
    Where an entry opens elsewhere, as the "Open in" badges of the browse
    screen show them. Built on the server, because the links encode the path
    in three different ways that the SampleDB and ElabFTW searches depend on;
    see `links.py`.
    """

    nextcloud: str | None = Field(default=None, description="Folders only.")
    jupyter: str | None = Field(default=None, description="Folders only.")
    imageviewer: str | None = Field(default=None, description="Files only.")
    libertem: str
    sampledb: str
    sampledb_erc3: str
    elabftw_erc3: str


class LocationAccess(EntryAccess):
    """The entry links plus the transfer commands for the browsed path."""

    rsync: str = Field(description="An rsync command that pushes to the path.")
    sftp: str = Field(description="An sftp command that opens the path's folder.")
    smb_windows: str = Field(description="The path on the Samba share, as a UNC path.")
    smb_unix: str = Field(description="The path on the Samba share, as an smb:// URL.")


class Entry(BaseModel):
    name: str
    path: str = Field(description="Normalized absolute path: what to browse to next.")
    isdir: bool
    mtime: float | None = Field(
        description=(
            "Modification time as epoch seconds; null if the file server reported none."
        )
    )
    size: int | None = Field(description="Size in bytes; null for a folder.")
    access: EntryAccess


class Browse(BaseModel):
    path: str = Field(
        description=(
            "The browsed path, normalized. A client that asked with another "
            "spelling (a trailing slash, a `..`) should switch to this one."
        )
    )
    name: str = Field(description="The last segment of `path`; empty for the root.")
    isdir: bool
    project: str | None = Field(
        description=(
            "The project the path is in: the second segment of `path`, where "
            "there is one."
        )
    )
    mtime: float | None
    size: int | None
    access: LocationAccess
    entries: list[Entry] = Field(
        description=(
            "One page of the folder's entries: folders first, then by name "
            "ignoring case. Empty for a file."
        )
    )
    total: int = Field(description="How many entries the folder has in all.")
    next_cursor: str | None = Field(
        description="Pass as `cursor` to get the page after this one; null on the last."
    )


# Sync `def`: the WebDAV call is blocking, so FastAPI runs it in a threadpool.
@router.get(
    "",
    summary="List a folder of the data tree, one page at a time",
    responses={
        **AUTH_ERROR_RESPONSES,
        **error_responses(
            {
                400: "The cursor is not one this endpoint issued.",
                403: "The signed-in user may not read that folder.",
                404: "There is no file or folder at that path.",
                502: "The file server did not answer.",
            }
        ),
    },
)
def read_path(
    request: Request,
    session: Annotated[SessionData, Depends(current_session)],
    path: Annotated[
        str,
        Query(
            description=(
                "Absolute path in the data tree, `/` for the root. It is "
                "normalized before use; the answer's `path` is that spelling."
            )
        ),
    ] = "/",
    limit: Annotated[
        int, Query(ge=1, le=MAX_LIMIT, description="Entries per page.")
    ] = DEFAULT_LIMIT,
    cursor: Annotated[
        str | None, Query(description="The `next_cursor` of the previous page.")
    ] = None,
) -> Browse:
    """
    The file or folder at `path`, with the links to open it elsewhere, and for
    a folder one page of its entries.

    Pages are keyed, not offset: the cursor marks the last entry of the page
    before, so the next page starts after it whatever was added to or removed
    from the folder in between, and no entry repeats or goes missing. The
    cursor is opaque, and it does not depend on `limit`.

    The listing is made as the signed-in user, so it shows what that user may
    see. The file server cannot page, so each page lists the whole folder on
    the server; that costs well under a second even for a folder of tens of
    thousands of entries.
    """
    settings = settings_of(request)
    username = session["username"]
    normalized = normalize_path(path)
    # Decoded before the listing, so that a bad cursor fails without one.
    after = _decode_cursor(cursor) if cursor is not None else None

    try:
        listing = list_path(settings, username, normalized)
    except NotFound as exc:
        raise APIError(
            404, "path_not_found", f"There is no file or folder at {normalized!r}."
        ) from exc
    except NoAccess as exc:
        raise APIError(
            403, "no_access", f"You do not have access to {normalized!r}."
        ) from exc

    entries = sorted(listing.entries, key=_sort_key)
    start = bisect_right(entries, after, key=_sort_key) if after is not None else 0
    page = entries[start : start + limit]
    next_cursor = _encode_cursor(page[-1]) if start + limit < len(entries) else None

    resource = listing.resource
    return Browse(
        path=resource.path,
        name=resource.name,
        isdir=resource.isdir,
        project=_project_of(resource.path),
        mtime=resource.mtime,
        size=resource.size,
        access=LocationAccess(
            **location_access(settings, resource.path, resource.isdir, username)
        ),
        entries=[
            Entry(
                name=entry.name,
                path=entry.path,
                isdir=entry.isdir,
                mtime=entry.mtime,
                size=entry.size,
                access=EntryAccess(**entry_access(settings, entry.path, entry.isdir)),
            )
            for entry in page
        ],
        total=len(entries),
        next_cursor=next_cursor,
    )


def _sort_key(resource: Resource) -> tuple[bool, str, str]:
    # Folders first, then by name ignoring case; the exact name breaks ties so
    # that the order is total, which the cursor relies on.
    return (not resource.isdir, resource.name.casefold(), resource.name)


def _project_of(path: str) -> str | None:
    # `/adhoc/<project>/...`: the second segment, as the legacy page read it.
    segments = path.strip("/").split("/")
    return segments[1] if len(segments) >= 2 else None


def _encode_cursor(last: Resource) -> str:
    # What the sort key is derived from, base64url without padding so that it
    # travels in a query string unchanged.
    raw = json.dumps([last.isdir, last.name]).encode("utf8")
    return base64.urlsafe_b64encode(raw).decode("ascii").rstrip("=")


def _decode_cursor(cursor: str) -> tuple[bool, str, str]:
    """The sort key the page after `cursor` starts behind."""
    try:
        padded = cursor + "=" * (-len(cursor) % 4)
        isdir, name = json.loads(base64.urlsafe_b64decode(padded).decode("utf8"))
        if not isinstance(isdir, bool) or not isinstance(name, str):
            raise ValueError("not a cursor")
    except ValueError as exc:
        # `binascii.Error`, `JSONDecodeError`, `UnicodeDecodeError` and an
        # unpacking of the wrong length are all `ValueError`s.
        raise APIError(
            400, "bad_cursor", "The cursor is not one this endpoint issued."
        ) from exc
    return (not isdir, name.casefold(), name)
