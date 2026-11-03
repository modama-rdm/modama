"""
The WebDAV calls and the `multistatus` parser.

The XML shapes are those wsgidav answered on the test VM on 26 Oct 2026: hrefs
percent-encoded, a folder's with a trailing slash, a file's `resourcetype`
empty, and a folder's `getcontentlength` reported in a second `propstat` with
status 404.
"""

from types import SimpleNamespace
from typing import Any
from urllib.parse import quote

import pytest
import requests
from mgmtinterface_api.config import Settings
from mgmtinterface_api.errors import APIError
from mgmtinterface_api.webdav import (
    NoAccess,
    NotAFile,
    NotFound,
    Resource,
    delete_file,
    list_path,
    normalize_path,
    parse_multistatus,
    put_file,
)

MODIFIED = "Fri, 02 Oct 2026 11:00:00 GMT"
MODIFIED_EPOCH = 1_790_938_800.0


def folder_response(path: str, href: str | None = None) -> str:
    href = quote(path) + "/" if href is None else href
    return (
        f"<D:response><D:href>{href}</D:href>"
        "<D:propstat><D:prop><D:resourcetype><D:collection/></D:resourcetype>"
        f"<D:getlastmodified>{MODIFIED}</D:getlastmodified></D:prop>"
        "<D:status>HTTP/1.1 200 OK</D:status></D:propstat>"
        "<D:propstat><D:prop><D:getcontentlength/></D:prop>"
        "<D:status>HTTP/1.1 404 Not Found</D:status></D:propstat>"
        "</D:response>"
    )


def file_response(path: str, size: int = 65536, modified: str = MODIFIED) -> str:
    return (
        f"<D:response><D:href>{quote(path)}</D:href>"
        "<D:propstat><D:prop><D:resourcetype></D:resourcetype>"
        f"<D:getcontentlength>{size}</D:getcontentlength>"
        f"<D:getlastmodified>{modified}</D:getlastmodified></D:prop>"
        "<D:status>HTTP/1.1 200 OK</D:status></D:propstat>"
        "</D:response>"
    )


def multistatus(*responses: str) -> bytes:
    return (
        "<?xml version='1.0' encoding='UTF-8'?>"
        '<D:multistatus xmlns:D="DAV:">' + "".join(responses) + "</D:multistatus>"
    ).encode("utf8")


@pytest.mark.parametrize(
    "raw,normalized",
    (
        ("/", "/"),
        ("", "/"),
        ("adhoc", "/adhoc"),
        ("/adhoc/proj/", "/adhoc/proj"),
        ("/adhoc//proj/./sess", "/adhoc/proj/sess"),
        ("/adhoc/proj/../other", "/adhoc/other"),
        # Cannot point above the root.
        ("/../..", "/"),
        ("/adhoc/../../etc", "/etc"),
    ),
)
def test_normalize_path(raw: str, normalized: str):
    assert normalize_path(raw) == normalized


def test_parses_a_folder_with_its_entries():
    body = multistatus(
        folder_response("/adhoc/dm3"),
        folder_response("/adhoc/dm3/2026-10-02 nested series"),
        file_response("/adhoc/dm3/with space.dat", size=0),
    )
    listing = parse_multistatus(body, "/adhoc/dm3")
    assert listing.resource == Resource(
        path="/adhoc/dm3", name="dm3", isdir=True, mtime=MODIFIED_EPOCH, size=None
    )
    assert listing.entries == [
        Resource(
            path="/adhoc/dm3/2026-10-02 nested series",
            name="2026-10-02 nested series",
            isdir=True,
            mtime=MODIFIED_EPOCH,
            size=None,
        ),
        Resource(
            path="/adhoc/dm3/with space.dat",
            name="with space.dat",
            isdir=False,
            mtime=MODIFIED_EPOCH,
            size=0,
        ),
    ]


def test_parses_a_file():
    body = multistatus(file_response("/adhoc/dm3/frame_000.tif"))
    listing = parse_multistatus(body, "/adhoc/dm3/frame_000.tif")
    assert listing.resource.isdir is False
    assert listing.resource.size == 65536
    assert listing.resource.name == "frame_000.tif"
    assert listing.entries == []


def test_decodes_hrefs_without_turning_plus_into_a_space():
    body = multistatus(
        folder_response("/raw"),
        file_response("/raw/plus+sign.dat"),
        file_response("/raw/hash#tag.dat"),
    )
    names = [entry.name for entry in parse_multistatus(body, "/raw").entries]
    assert names == ["plus+sign.dat", "hash#tag.dat"]


def test_finds_the_folder_itself_by_href_not_by_position():
    # Also when the server spells the href as a full URL.
    body = multistatus(
        file_response("/raw/a.dat"),
        folder_response("/raw", href="http://webdav:8080/raw/"),
    )
    listing = parse_multistatus(body, "/raw")
    assert listing.resource.path == "/raw"
    assert [entry.name for entry in listing.entries] == ["a.dat"]


def test_root_has_an_empty_name():
    listing = parse_multistatus(multistatus(folder_response("/", href="/")), "/")
    assert listing.resource.path == "/"
    assert listing.resource.name == ""


def test_missing_or_unreadable_dates_are_null():
    body = multistatus(
        folder_response("/raw"),
        file_response("/raw/a.dat", modified="not a date"),
    )
    assert parse_multistatus(body, "/raw").entries[0].mtime is None


def test_an_answer_that_does_not_describe_the_path_is_an_error():
    with pytest.raises(ValueError):
        parse_multistatus(multistatus(file_response("/raw/a.dat")), "/raw")


@pytest.fixture
def webdav(monkeypatch: pytest.MonkeyPatch) -> dict[str, Any]:
    """
    A fake `requests.request` that answers what `reply` holds and records the
    call. Replaced per test; this module makes every WebDAV request.
    """
    state: dict[str, Any] = {"reply": None, "calls": []}

    def fake_request(method: str, url: str, **kwargs: Any) -> SimpleNamespace:
        state["calls"].append({"method": method, "url": url, **kwargs})
        if isinstance(state["reply"], Exception):
            raise state["reply"]
        return state["reply"]

    monkeypatch.setattr("mgmtinterface_api.webdav.requests.request", fake_request)
    return state


def test_makes_one_propfind_as_the_user(settings: Settings, webdav: dict[str, Any]):
    webdav["reply"] = SimpleNamespace(
        status_code=207,
        content=multistatus(folder_response("/adhoc/my proj#1")),
    )
    listing = list_path(settings, "shanker", "/adhoc/my proj#1")
    assert listing.resource.name == "my proj#1"

    (call,) = webdav["calls"]
    assert call["method"] == "PROPFIND"
    # Percent-encoded, with `#` as data and the configured server in front.
    assert call["url"] == "http://webdav:8080/adhoc/my%20proj%231"
    assert call["auth"][0] == "shanker"
    assert call["headers"]["Depth"] == "1"
    assert call["timeout"] == settings.webdav_timeout_seconds
    # Named properties, not allprop: half the XML for a big folder.
    assert "<D:allprop/>" not in call["data"]
    assert "<D:getlastmodified/>" in call["data"]


def test_missing_path(settings: Settings, webdav: dict[str, Any]):
    webdav["reply"] = SimpleNamespace(status_code=404, content=b"404 Not Found")
    with pytest.raises(NotFound):
        list_path(settings, "shanker", "/nope")


def test_unreadable_folder(settings: Settings, webdav: dict[str, Any]):
    webdav["reply"] = SimpleNamespace(status_code=403, content=b"permission error")
    with pytest.raises(NoAccess):
        list_path(settings, "shanker", "/private")


@pytest.mark.parametrize(
    "reply",
    (
        requests.ConnectionError("webdav is down"),
        requests.Timeout("webdav is slow"),
        SimpleNamespace(status_code=500, content=b"Internal Server Error"),
        SimpleNamespace(status_code=207, content=b"<not xml"),
        # A 207 that does not describe the path asked for.
        SimpleNamespace(status_code=207, content=multistatus(file_response("/x"))),
    ),
)
def test_anything_else_is_unavailable(
    settings: Settings, webdav: dict[str, Any], reply: Any
):
    webdav["reply"] = reply
    with pytest.raises(APIError) as excinfo:
        list_path(settings, "shanker", "/adhoc")
    assert excinfo.value.status_code == 502
    assert excinfo.value.code == "files_unavailable"


# --- put_file and delete_file (#112) -----------------------------------------
#
# The statuses are those of the suid wsgidav: a `PermissionError` anywhere is a
# 403, a PUT into a missing folder a 409, a PUT onto a folder a 405, and a
# DELETE of a folder with `Depth: 0` a 400.

README = "/adhoc/my proj#1/README.md"


def reply(status: int) -> SimpleNamespace:
    return SimpleNamespace(status_code=status, content=b"")


def test_puts_the_bytes_as_the_user(settings: Settings, webdav: dict[str, Any]):
    webdav["reply"] = reply(201)
    put_file(settings, "shanker", README, b"text\n")

    (call,) = webdav["calls"]
    assert call["method"] == "PUT"
    assert call["url"] == "http://webdav:8080/adhoc/my%20proj%231/README.md"
    assert call["auth"][0] == "shanker"
    assert call["data"] == b"text\n"
    assert call["timeout"] == settings.webdav_timeout_seconds


@pytest.mark.parametrize("status", [200, 201, 204])
def test_a_put_succeeds(settings: Settings, webdav: dict[str, Any], status: int):
    webdav["reply"] = reply(status)
    put_file(settings, "shanker", README, b"x")


@pytest.mark.parametrize(
    ("status", "error"),
    [(403, NoAccess), (404, NotFound), (409, NotFound), (405, NotAFile)],
)
def test_a_refused_put(
    settings: Settings, webdav: dict[str, Any], status: int, error: type
):
    webdav["reply"] = reply(status)
    with pytest.raises(error):
        put_file(settings, "shanker", README, b"x")


def test_deletes_with_depth_zero_as_the_user(
    settings: Settings, webdav: dict[str, Any]
):
    webdav["reply"] = reply(204)
    delete_file(settings, "shanker", README)

    (call,) = webdav["calls"]
    assert call["method"] == "DELETE"
    assert call["url"] == "http://webdav:8080/adhoc/my%20proj%231/README.md"
    assert call["auth"][0] == "shanker"
    # Never `infinity`, which would take a folder with everything in it.
    assert call["headers"] == {"Depth": "0"}


@pytest.mark.parametrize(
    ("status", "error"),
    [(403, NoAccess), (404, NotFound), (400, NotAFile)],
)
def test_a_refused_delete(
    settings: Settings, webdav: dict[str, Any], status: int, error: type
):
    webdav["reply"] = reply(status)
    with pytest.raises(error):
        delete_file(settings, "shanker", README)


@pytest.mark.parametrize("call", [put_file, delete_file])
@pytest.mark.parametrize(
    "answer",
    [requests.ConnectionError("webdav is down"), reply(500), reply(207)],
)
def test_any_other_answer_to_a_change_is_unavailable(
    settings: Settings, webdav: dict[str, Any], call: Any, answer: Any
):
    webdav["reply"] = answer
    args = (b"x",) if call is put_file else ()
    with pytest.raises(APIError) as excinfo:
        call(settings, "shanker", README, *args)
    assert excinfo.value.status_code == 502
    assert excinfo.value.code == "files_unavailable"
