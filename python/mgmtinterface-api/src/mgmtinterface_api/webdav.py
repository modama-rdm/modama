"""
Calls into the WebDAV server in front of the data tree: listing a path, for
browsing, and writing or removing a single file, for descriptions (#112).

One `PROPFIND` with `Depth: 1` answers everything the browse endpoint needs: the
properties of the path itself and, for a folder, of each entry in it. The
legacy browse page made three such requests per folder (two of them full
listings) through `webdavclient3`; this module makes one, asks for three
properties instead of `allprop`, and needs no WebDAV library.

Every request is made as the signed-in user. The server (the `suid` fork of
wsgidav) authenticates by username alone and switches to that user's Unix
identity for the request, so the file system decides what the user sees and
what they may change, as it does over SMB. That trust model is the same as the
Flask app's; the server is only reachable inside the compose network.
"""

import posixpath
from collections.abc import Iterator
from dataclasses import dataclass
from email.utils import parsedate_to_datetime
from typing import Any
from urllib.parse import quote, unquote, urlsplit
from xml.etree import ElementTree

import requests

from .config import Settings
from .errors import APIError

#: The server ignores the password (`useronly`), but HTTP Basic needs one.
_USERONLY_PASSWORD = "-"

_DAV = "{DAV:}"

#: Only what the browse response reports. `allprop` would double the size of
#: the answer with lock and display-name properties nothing reads.
_PROPFIND_BODY = (
    '<?xml version="1.0" encoding="utf-8"?>'
    '<D:propfind xmlns:D="DAV:"><D:prop>'
    "<D:resourcetype/><D:getcontentlength/><D:getlastmodified/>"
    "</D:prop></D:propfind>"
)


class NotFound(Exception):
    """There is no file or folder at the path."""


class NoAccess(Exception):
    """The server refused to list or change the path as this user."""


class NotAFile(Exception):
    """There is a folder where a file was expected."""


@dataclass(frozen=True, slots=True)
class Resource:
    #: Normalized absolute path, see `normalize_path`.
    path: str
    name: str
    isdir: bool
    #: Modification time as epoch seconds, or None if the server reported none.
    mtime: float | None
    #: None for a folder.
    size: int | None


@dataclass(frozen=True, slots=True)
class Listing:
    resource: Resource
    #: The folder's entries, in the server's order; empty for a file.
    entries: list[Resource]


def normalize_path(path: str) -> str:
    """
    The one spelling of a path: absolute, no trailing slash, no `.` or `..`
    components, no doubled slashes. `/a/../..` becomes `/`, so a path cannot
    point above the root.
    """
    return posixpath.normpath("/" + path.lstrip("/"))


def list_path(settings: Settings, username: str, path: str) -> Listing:
    """
    The resource at `path` and, for a folder, its entries, as `username` sees
    them. `path` must be normalized.
    """
    response = _request(
        settings,
        username,
        "PROPFIND",
        path,
        headers={"Depth": "1", "Content-Type": "application/xml"},
        data=_PROPFIND_BODY,
    )
    if response.status_code == 404:
        raise NotFound(path)
    if response.status_code == 403:
        raise NoAccess(path)
    if response.status_code != 207:
        raise _unavailable()
    try:
        return parse_multistatus(response.content, path)
    except (ElementTree.ParseError, ValueError) as exc:
        raise _unavailable() from exc


def put_file(settings: Settings, username: str, path: str, content: bytes) -> None:
    """
    Write `content` to the file at `path`, as `username`. `path` must be
    normalized.

    A new file belongs to `username`; an existing one is truncated and
    rewritten in place, so it keeps its owner. The server refuses any path
    through a symlink (it fails with a 500), and it would block on a FIFO; a
    caller that may meet one checks first.
    """
    response = _request(settings, username, "PUT", path, data=content)
    if response.status_code in (200, 201, 204):
        return
    if response.status_code == 403:
        raise NoAccess(path)
    # 409: the folder the file would go into is missing.
    if response.status_code in (404, 409):
        raise NotFound(path)
    if response.status_code == 405:
        raise NotAFile(path)
    raise _unavailable()


def delete_file(settings: Settings, username: str, path: str) -> None:
    """
    Remove the file at `path`, as `username`. `path` must be normalized.

    `Depth: 0` makes the server refuse a folder with a 400, where `infinity`,
    the default, would remove it with everything in it.
    """
    response = _request(settings, username, "DELETE", path, headers={"Depth": "0"})
    if response.status_code in (200, 204):
        return
    if response.status_code == 403:
        raise NoAccess(path)
    if response.status_code == 404:
        raise NotFound(path)
    if response.status_code == 400:
        raise NotAFile(path)
    raise _unavailable()


def _request(
    settings: Settings, username: str, method: str, path: str, **kwargs: Any
) -> requests.Response:
    try:
        return requests.request(
            method,
            settings.webdav_api.rstrip("/") + quote(path),
            auth=(username, _USERONLY_PASSWORD),
            timeout=settings.webdav_timeout_seconds,
            **kwargs,
        )
    except requests.RequestException as exc:
        raise _unavailable() from exc


def parse_multistatus(body: bytes, path: str) -> Listing:
    """
    The `multistatus` answer to a `Depth: 1` request for `path`.

    The folder's own entry is found by its href, not by position. Each
    property is read from the `propstat` that reports it with status 200; a
    folder, for instance, answers `getcontentlength` in a second `propstat`
    with status 404. The XML comes from our own server, so the standard
    library parser is fine here.
    """
    resource = None
    entries = []
    for element in ElementTree.fromstring(body).iter(f"{_DAV}response"):
        href = element.findtext(f"{_DAV}href")
        if href is None:
            continue
        found = _resource_from(href, element)
        if found.path == path:
            resource = found
        else:
            entries.append(found)
    if resource is None:
        raise ValueError(f"The answer does not describe {path!r}.")
    return Listing(resource, entries if resource.isdir else [])


def _resource_from(href: str, response: ElementTree.Element) -> Resource:
    # hrefs are percent-encoded, a folder's ends with a slash, and the server
    # may spell them as full URLs. `unquote`, not `unquote_plus`: a `+` in a
    # file name is a `+`.
    path = normalize_path(unquote(urlsplit(href).path))
    isdir = False
    mtime = None
    size = None
    for prop in _reported_properties(response):
        if prop.tag == f"{_DAV}resourcetype":
            isdir = prop.find(f"{_DAV}collection") is not None
        elif prop.tag == f"{_DAV}getcontentlength" and prop.text:
            size = int(prop.text)
        elif prop.tag == f"{_DAV}getlastmodified" and prop.text:
            mtime = _epoch_seconds(prop.text)
    return Resource(
        path=path,
        name=posixpath.basename(path),
        isdir=isdir,
        mtime=mtime,
        size=None if isdir else size,
    )


def _reported_properties(
    response: ElementTree.Element,
) -> Iterator[ElementTree.Element]:
    for propstat in response.iter(f"{_DAV}propstat"):
        status = propstat.findtext(f"{_DAV}status") or ""
        if " 200 " not in f" {status} ":
            continue
        prop = propstat.find(f"{_DAV}prop")
        if prop is not None:
            yield from prop


def _epoch_seconds(http_date: str) -> float | None:
    # `getlastmodified` is an RFC 1123 date, the HTTP format.
    try:
        return parsedate_to_datetime(http_date).timestamp()
    except (TypeError, ValueError):
        return None


def _unavailable() -> APIError:
    return APIError(
        502,
        "files_unavailable",
        "The file server is unavailable. Please try again shortly.",
    )
