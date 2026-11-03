"""
Writing a description: README.md over WebDAV, only when it is absent or a
plain file.

The WebDAV calls are faked where `descriptions` looks them up. The fake folder
listing behaves as wsgidav's does: anything that is neither a file nor a
folder, such as a FIFO, is left out of the listing but answers a request of
its own. (A symlink answers neither way: the suid fork refuses it, so to the
API it is absent.)
"""

from typing import Any

import pytest
from mgmtinterface_api.config import Settings
from mgmtinterface_api.descriptions import (
    NotAPlainFile,
    normalize_description,
    project_folder,
    session_folder,
    write_description,
)
from mgmtinterface_api.webdav import (
    Listing,
    NoAccess,
    NotAFile,
    NotFound,
    Resource,
)

FOLDER = "/adhoc/proj/run 1"
README = f"{FOLDER}/README.md"


def resource(path: str, isdir: bool) -> Resource:
    return Resource(
        path=path,
        name=path.rsplit("/", 1)[-1],
        isdir=isdir,
        mtime=None,
        size=None if isdir else 3,
    )


@pytest.fixture
def files(monkeypatch: pytest.MonkeyPatch) -> dict[str, Any]:
    """
    One session folder with `raw/` in it. `readme` says what README.md is:
    None (absent), "file", "folder", or "hidden" (a FIFO).
    `put_error` and `delete_error` make those calls fail; `calls` records all
    of them.
    """
    state: dict[str, Any] = {
        "readme": None,
        "folder_missing": False,
        "put_error": None,
        "delete_error": None,
        "calls": [],
    }

    def list_path(settings: Settings, username: str, path: str) -> Listing:
        state["calls"].append(("PROPFIND", username, path))
        if path == FOLDER:
            if state["folder_missing"]:
                raise NotFound(path)
            entries = [resource(f"{FOLDER}/raw", isdir=True)]
            if state["readme"] in ("file", "folder"):
                entries.append(resource(README, isdir=state["readme"] == "folder"))
            return Listing(resource(FOLDER, isdir=True), entries)
        assert path == README
        if state["readme"] is None:
            raise NotFound(path)
        return Listing(resource(README, isdir=state["readme"] == "folder"), [])

    def put_file(settings: Settings, username: str, path: str, content: bytes):
        state["calls"].append(("PUT", username, path, content))
        if state["put_error"] is not None:
            raise state["put_error"]

    def delete_file(settings: Settings, username: str, path: str):
        state["calls"].append(("DELETE", username, path))
        if state["delete_error"] is not None:
            raise state["delete_error"]

    monkeypatch.setattr("mgmtinterface_api.descriptions.list_path", list_path)
    monkeypatch.setattr("mgmtinterface_api.descriptions.put_file", put_file)
    monkeypatch.setattr("mgmtinterface_api.descriptions.delete_file", delete_file)
    return state


def changes(files: dict[str, Any]) -> list[tuple]:
    return [call for call in files["calls"] if call[0] != "PROPFIND"]


def test_a_missing_readme_is_created(settings: Settings, files: dict[str, Any]):
    write_description(settings, "shanker", FOLDER, "# Run 1\nä")
    assert files["calls"] == [
        ("PROPFIND", "shanker", FOLDER),
        # Not listed: asked on its own, to tell absent from hidden.
        ("PROPFIND", "shanker", README),
        # UTF-8, with one trailing newline.
        ("PUT", "shanker", README, "# Run 1\nä\n".encode()),
    ]


def test_a_plain_readme_is_rewritten(settings: Settings, files: dict[str, Any]):
    files["readme"] = "file"
    write_description(settings, "shanker", FOLDER, "new")
    assert files["calls"] == [
        ("PROPFIND", "shanker", FOLDER),
        ("PUT", "shanker", README, b"new\n"),
    ]


def test_a_plain_readme_is_removed(settings: Settings, files: dict[str, Any]):
    files["readme"] = "file"
    write_description(settings, "shanker", FOLDER, None)
    assert changes(files) == [("DELETE", "shanker", README)]


def test_removing_a_missing_readme_changes_nothing(
    settings: Settings, files: dict[str, Any]
):
    write_description(settings, "shanker", FOLDER, None)
    assert changes(files) == []


@pytest.mark.parametrize("readme", ["folder", "hidden"])
@pytest.mark.parametrize("text", ["new", None])
def test_anything_but_a_plain_file_is_left_alone(
    settings: Settings, files: dict[str, Any], readme: str, text: str | None
):
    # Writing to a FIFO would block the server; a folder is no file.
    files["readme"] = readme
    with pytest.raises(NotAPlainFile):
        write_description(settings, "shanker", FOLDER, text)
    assert changes(files) == []


def test_a_missing_folder_is_not_found(settings: Settings, files: dict[str, Any]):
    files["folder_missing"] = True
    with pytest.raises(NotFound):
        write_description(settings, "shanker", FOLDER, "x")
    assert changes(files) == []


def test_a_readme_removed_meanwhile_is_not_an_error(
    settings: Settings, files: dict[str, Any]
):
    files["readme"] = "file"
    files["delete_error"] = NotFound(README)
    write_description(settings, "shanker", FOLDER, None)


def test_a_folder_removed_before_the_put_is_not_found(
    settings: Settings, files: dict[str, Any]
):
    files["readme"] = "file"
    files["put_error"] = NotFound(README)
    with pytest.raises(NotFound):
        write_description(settings, "shanker", FOLDER, "x")


@pytest.mark.parametrize("text", ["new", None])
def test_a_folder_swapped_in_after_the_check_is_not_a_plain_file(
    settings: Settings, files: dict[str, Any], text: str | None
):
    files["readme"] = "file"
    files["put_error"] = files["delete_error"] = NotAFile(README)
    with pytest.raises(NotAPlainFile):
        write_description(settings, "shanker", FOLDER, text)


def test_a_refusal_of_the_file_system_is_passed_on(
    settings: Settings, files: dict[str, Any]
):
    files["readme"] = "file"
    files["put_error"] = NoAccess(README)
    with pytest.raises(NoAccess):
        write_description(settings, "shanker", FOLDER, "not mine")


@pytest.mark.parametrize(
    ("text", "normalized"),
    [
        (None, None),
        ("", None),
        (" \r\n\t", None),
        ("  # Title\r\n\r\nbody  \n", "# Title\n\nbody"),
    ],
)
def test_normalize_description(text: str | None, normalized: str | None):
    assert normalize_description(text) == normalized


def test_folders_in_the_data_tree():
    assert project_folder("adhoc/proj") == "/adhoc/proj"
    assert session_folder("adhoc/proj", "run 1") == "/adhoc/proj/run 1"
