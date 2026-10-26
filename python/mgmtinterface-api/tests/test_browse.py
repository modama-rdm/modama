"""
The browse endpoint, through the app, against a fake WebDAV server.

The fake answers `PROPFIND` from a small tree in the XML shapes wsgidav uses,
so these tests go through the real parser as well; `test_webdav.py` has the
parser's own cases.
"""

import base64
import posixpath
from types import SimpleNamespace
from typing import Any
from urllib.parse import quote, unquote, urlsplit

import pytest
import requests
from conftest import api_client, signed_in_client
from fastapi.testclient import TestClient
from mgmtinterface_api import API_PREFIX, create_app
from mgmtinterface_api.config import Settings
from mgmtinterface_api.webdav import normalize_path

BROWSE = f"{API_PREFIX}/browse"
MODIFIED = "Fri, 02 Oct 2026 11:00:00 GMT"
MODIFIED_EPOCH = 1_790_938_800.0


class FakeWebDAV:
    """A data tree that answers `PROPFIND` with `Depth: 1` the way wsgidav does."""

    def __init__(self) -> None:
        self.folders: set[str] = {"/"}
        self.files: dict[str, int] = {}
        self.forbidden: set[str] = set()
        #: Set to an exception to raise or a status code to answer instead.
        self.failure: Exception | int | None = None
        self.calls: list[dict[str, Any]] = []

    def folder(self, path: str) -> "FakeWebDAV":
        self.folders.add(path)
        return self

    def file(self, path: str, size: int = 0) -> "FakeWebDAV":
        self.files[path] = size
        return self

    def request(self, method: str, url: str, **kwargs: Any) -> SimpleNamespace:
        self.calls.append({"method": method, "url": url, **kwargs})
        if isinstance(self.failure, Exception):
            raise self.failure
        if self.failure is not None:
            return SimpleNamespace(status_code=self.failure, content=b"")
        path = normalize_path(unquote(urlsplit(url).path))
        if path in self.forbidden:
            return SimpleNamespace(status_code=403, content=b"permission error")
        if path in self.folders:
            responses = [self._folder(path)]
            for child in sorted(self.folders):
                if child != path and posixpath.dirname(child) == path:
                    responses.append(self._folder(child))
            for child, size in self.files.items():
                if posixpath.dirname(child) == path:
                    responses.append(self._file(child, size))
            return SimpleNamespace(status_code=207, content=_multistatus(responses))
        if path in self.files:
            body = _multistatus([self._file(path, self.files[path])])
            return SimpleNamespace(status_code=207, content=body)
        return SimpleNamespace(status_code=404, content=b"404 Not Found")

    @staticmethod
    def _folder(path: str) -> str:
        href = "/" if path == "/" else quote(path) + "/"
        return (
            f"<D:response><D:href>{href}</D:href>"
            "<D:propstat><D:prop><D:resourcetype><D:collection/></D:resourcetype>"
            f"<D:getlastmodified>{MODIFIED}</D:getlastmodified></D:prop>"
            "<D:status>HTTP/1.1 200 OK</D:status></D:propstat>"
            "<D:propstat><D:prop><D:getcontentlength/></D:prop>"
            "<D:status>HTTP/1.1 404 Not Found</D:status></D:propstat>"
            "</D:response>"
        )

    @staticmethod
    def _file(path: str, size: int) -> str:
        return (
            f"<D:response><D:href>{quote(path)}</D:href>"
            "<D:propstat><D:prop><D:resourcetype></D:resourcetype>"
            f"<D:getcontentlength>{size}</D:getcontentlength>"
            f"<D:getlastmodified>{MODIFIED}</D:getlastmodified></D:prop>"
            "<D:status>HTTP/1.1 200 OK</D:status></D:propstat>"
            "</D:response>"
        )


def _multistatus(responses: list[str]) -> bytes:
    return (
        "<?xml version='1.0' encoding='UTF-8'?>"
        '<D:multistatus xmlns:D="DAV:">' + "".join(responses) + "</D:multistatus>"
    ).encode("utf8")


@pytest.fixture
def webdav(monkeypatch: pytest.MonkeyPatch) -> FakeWebDAV:
    fake = FakeWebDAV()
    monkeypatch.setattr("mgmtinterface_api.webdav.requests.request", fake.request)
    return fake


@pytest.fixture
def tree(webdav: FakeWebDAV) -> FakeWebDAV:
    """A project with one session: three folders and four files in it."""
    session = "/adhoc/proj/sess"
    webdav.folder("/adhoc").folder("/adhoc/proj").folder(session)
    for folder in ("raw", "Dir_A", "dir_b"):
        webdav.folder(f"{session}/{folder}")
    webdav.file(f"{session}/B.dat", 10)
    webdav.file(f"{session}/a.dat", 20)
    webdav.file(f"{session}/A.dat", 30)
    webdav.file(f"{session}/with space.dat", 0)
    return webdav


@pytest.fixture
def client(settings: Settings, webdav: FakeWebDAV) -> TestClient:
    return signed_in_client(create_app(settings), settings, "shanker")


def names(body: dict[str, Any]) -> list[str]:
    return [entry["name"] for entry in body["entries"]]


def test_lists_a_folder(client: TestClient, tree: FakeWebDAV):
    response = client.get(BROWSE, params={"path": "/adhoc/proj/sess"})
    assert response.status_code == 200
    body = response.json()

    assert body["path"] == "/adhoc/proj/sess"
    assert body["name"] == "sess"
    assert body["isdir"] is True
    assert body["project"] == "proj"
    assert body["mtime"] == MODIFIED_EPOCH
    assert body["size"] is None
    assert body["total"] == 7
    assert body["next_cursor"] is None
    # Folders first, then by name ignoring case, the exact name as tiebreaker.
    # No `..` row: the client knows the parent from `path`.
    assert names(body) == [
        "Dir_A",
        "dir_b",
        "raw",
        "A.dat",
        "a.dat",
        "B.dat",
        "with space.dat",
    ]

    folder = body["entries"][0]
    assert folder["path"] == "/adhoc/proj/sess/Dir_A"
    assert folder["isdir"] is True
    assert folder["size"] is None
    assert folder["mtime"] == MODIFIED_EPOCH
    assert folder["access"]["nextcloud"] is not None
    assert folder["access"]["imageviewer"] is None

    file = body["entries"][5]
    assert file["name"] == "B.dat"
    assert file["size"] == 10
    assert file["access"]["imageviewer"] is not None
    assert file["access"]["nextcloud"] is None
    assert "with+space.dat" in body["entries"][6]["access"]["libertem"]

    assert body["access"]["rsync"].startswith("rsync ")
    assert body["access"]["smb_windows"].endswith("\\adhoc\\proj\\sess")
    assert body["access"]["imageviewer"] is None


def test_describes_a_file(client: TestClient, tree: FakeWebDAV):
    response = client.get(BROWSE, params={"path": "/adhoc/proj/sess/B.dat"})
    assert response.status_code == 200
    body = response.json()
    assert body["isdir"] is False
    assert body["name"] == "B.dat"
    assert body["size"] == 10
    assert body["project"] == "proj"
    assert body["entries"] == []
    assert body["total"] == 0
    assert body["next_cursor"] is None
    # A file opens in the image viewer, and its folder in Nextcloud.
    assert body["access"]["imageviewer"] is not None
    assert body["access"]["nextcloud"].endswith("%2Fadhoc%2Fproj%2Fsess")


def test_root_is_the_default(client: TestClient, tree: FakeWebDAV):
    response = client.get(BROWSE)
    assert response.status_code == 200
    body = response.json()
    assert body["path"] == "/"
    assert body["name"] == ""
    assert body["project"] is None
    assert names(body) == ["adhoc"]


@pytest.mark.parametrize(
    "path,project",
    (
        ("/", None),
        ("/adhoc", None),
        ("/adhoc/proj", "proj"),
        ("/adhoc/proj/sess/raw", "proj"),
    ),
)
def test_project_is_the_second_segment(
    client: TestClient, tree: FakeWebDAV, path: str, project: str | None
):
    response = client.get(BROWSE, params={"path": path})
    assert response.status_code == 200
    assert response.json()["project"] == project


@pytest.mark.parametrize(
    "raw,normalized",
    (
        ("/adhoc//proj/../proj/sess/", "/adhoc/proj/sess"),
        ("adhoc/proj", "/adhoc/proj"),
        ("/../../adhoc", "/adhoc"),
    ),
)
def test_normalizes_the_path(
    client: TestClient, tree: FakeWebDAV, raw: str, normalized: str
):
    response = client.get(BROWSE, params={"path": raw})
    assert response.status_code == 200
    assert response.json()["path"] == normalized
    # The file server was asked for the normalized path, percent-encoded.
    assert tree.calls[-1]["url"] == "http://webdav:8080" + quote(normalized)


def test_lists_as_the_signed_in_user(client: TestClient, tree: FakeWebDAV):
    client.get(BROWSE, params={"path": "/adhoc"})
    (call,) = tree.calls
    assert call["method"] == "PROPFIND"
    assert call["auth"][0] == "shanker"
    assert call["headers"]["Depth"] == "1"


@pytest.fixture
def big(webdav: FakeWebDAV) -> FakeWebDAV:
    """A folder with a subfolder and seven files, f1 to f7."""
    webdav.folder("/big").folder("/big/sub")
    for n in range(1, 8):
        webdav.file(f"/big/f{n}.dat", n)
    return webdav


def test_pages_with_a_cursor(client: TestClient, big: FakeWebDAV):
    seen: list[str] = []
    params: dict[str, Any] = {"path": "/big", "limit": 3}
    pages = 0
    while True:
        response = client.get(BROWSE, params=params)
        assert response.status_code == 200
        body = response.json()
        pages += 1
        seen.extend(names(body))
        assert body["total"] == 8
        assert len(body["entries"]) <= 3
        if body["next_cursor"] is None:
            break
        params["cursor"] = body["next_cursor"]
    assert pages == 3
    assert seen == [
        "sub",
        "f1.dat",
        "f2.dat",
        "f3.dat",
        "f4.dat",
        "f5.dat",
        "f6.dat",
        "f7.dat",
    ]


def test_cursor_does_not_depend_on_limit(client: TestClient, big: FakeWebDAV):
    first = client.get(BROWSE, params={"path": "/big", "limit": 2}).json()
    assert names(first) == ["sub", "f1.dat"]
    rest = client.get(
        BROWSE, params={"path": "/big", "limit": 10, "cursor": first["next_cursor"]}
    ).json()
    assert names(rest) == ["f2.dat", "f3.dat", "f4.dat", "f5.dat", "f6.dat", "f7.dat"]
    assert rest["next_cursor"] is None


def test_an_entry_deleted_between_pages_hides_nothing(
    client: TestClient, big: FakeWebDAV
):
    first = client.get(BROWSE, params={"path": "/big", "limit": 3}).json()
    assert names(first) == ["sub", "f1.dat", "f2.dat"]
    # The entry the cursor marks is gone by the time the next page is asked.
    del big.files["/big/f2.dat"]
    second = client.get(
        BROWSE, params={"path": "/big", "limit": 3, "cursor": first["next_cursor"]}
    ).json()
    assert names(second) == ["f3.dat", "f4.dat", "f5.dat"]
    assert second["total"] == 7


def test_an_entry_added_between_pages_repeats_nothing(
    client: TestClient, big: FakeWebDAV
):
    first = client.get(BROWSE, params={"path": "/big", "limit": 3}).json()
    # An ingest writes a file that sorts before the page boundary. With an
    # offset, f2.dat would show up again on the next page.
    big.file("/big/f0.dat")
    second = client.get(
        BROWSE, params={"path": "/big", "limit": 3, "cursor": first["next_cursor"]}
    ).json()
    assert names(second) == ["f3.dat", "f4.dat", "f5.dat"]
    assert second["total"] == 9


@pytest.mark.parametrize(
    "cursor",
    (
        "not base64!!",
        base64.urlsafe_b64encode(b"[1, 2]").decode().rstrip("="),
        base64.urlsafe_b64encode(b'"just a string"').decode().rstrip("="),
        base64.urlsafe_b64encode(b"[true, 7]").decode().rstrip("="),
        base64.urlsafe_b64encode(b"[true]").decode().rstrip("="),
    ),
)
def test_rejects_a_cursor_it_did_not_issue(
    client: TestClient, big: FakeWebDAV, cursor: str
):
    response = client.get(BROWSE, params={"path": "/big", "cursor": cursor})
    assert response.status_code == 400
    assert response.json()["code"] == "bad_cursor"
    # Rejected before the file server was asked.
    assert big.calls == []


@pytest.mark.parametrize("limit,status", ((0, 422), (1, 200), (1000, 200), (1001, 422)))
def test_limit_bounds(client: TestClient, big: FakeWebDAV, limit: int, status: int):
    response = client.get(BROWSE, params={"path": "/big", "limit": limit})
    assert response.status_code == status


def test_missing_path(client: TestClient, tree: FakeWebDAV):
    response = client.get(BROWSE, params={"path": "/adhoc/nope"})
    assert response.status_code == 404
    assert response.json() == {
        "code": "path_not_found",
        "message": "There is no file or folder at '/adhoc/nope'.",
    }


def test_unreadable_folder(client: TestClient, tree: FakeWebDAV):
    tree.forbidden.add("/adhoc/proj/sess/raw")
    response = client.get(BROWSE, params={"path": "/adhoc/proj/sess/raw"})
    assert response.status_code == 403
    assert response.json()["code"] == "no_access"


@pytest.mark.parametrize(
    "failure", (requests.ConnectionError("webdav is down"), requests.Timeout(), 500)
)
def test_file_server_not_answering(
    client: TestClient, tree: FakeWebDAV, failure: Exception | int
):
    tree.failure = failure
    response = client.get(BROWSE, params={"path": "/adhoc"})
    assert response.status_code == 502
    assert response.json()["code"] == "files_unavailable"


def test_requires_a_session(settings: Settings, tree: FakeWebDAV):
    response = api_client(create_app(settings)).get(BROWSE, params={"path": "/adhoc"})
    assert response.status_code == 401
    assert response.json()["code"] == "not_authenticated"
    assert tree.calls == []
