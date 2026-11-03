"""
Descriptions: the text of a `README.md` in a project or session folder.

The permissions service reads the file, with every listing. Changes go through
the WebDAV server as the signed-in user, the way a person edits any other
file, so the file system decides who may make them; there is no rule of ours.
A new file belongs to whoever writes it first, and the watchdog leaves it
writable by that person only. Anyone who may write to the folder can add a
missing README; the sticky bit on the folder lets only the file's owner and
the folder's owner remove one.

The one check made here guards against what someone else may have put in the
file's place: the server would block writing to a FIFO named README.md, and a
folder of that name is no file at all. So README.md is only written or removed
when it is absent or a plain file. The check and the write are two requests,
so this narrows the window rather than closing it.

Symlinks need no check of ours: the server (the suid fork) refuses any path
through one and answers as if nothing were there. A symlinked README.md reads
as no description and an empty text leaves it alone; writing to it fails in
the server (a 500, so a 502 here), and nothing is written.
"""

import posixpath

from .config import Settings
from .errors import APIError
from .webdav import NoAccess, NotAFile, NotFound, delete_file, list_path, put_file

README_NAME = "README.md"

#: In characters, which is what JSON schema's `maxLength` counts. The
#: permissions service reads back at most 65 536 characters of the file.
DESCRIPTION_MAX_LENGTH = 10_000


def normalize_description(text: str | None) -> str | None:
    """Line ends as LF, surrounding whitespace dropped, blank as None."""
    if text is None:
        return None
    text = text.replace("\r\n", "\n").strip()
    return text or None


class NotAPlainFile(Exception):
    """README.md is there, but as a folder or a special file such as a FIFO."""


def project_folder(relpath: str) -> str:
    """The project folder in the data tree, from the project's `relpath`."""
    return posixpath.normpath("/" + relpath)


def session_folder(relpath: str, session: str) -> str:
    """A session folder in the data tree, from its project's `relpath`."""
    return posixpath.join(project_folder(relpath), session)


def write_description(
    settings: Settings, username: str, folder: str, text: str | None
) -> None:
    """
    Save `text`, which must be normalized, as README.md in `folder`, as
    `username`; None removes the file, and no file is not an error. `folder`
    is a normalized path in the data tree.

    Raises `NotFound` when the folder is missing, `NoAccess` when the file
    system refuses, and `NotAPlainFile` for anything but a plain file.
    """
    path = posixpath.join(folder, README_NAME)
    exists = _plain_readme_exists(settings, username, folder, path)
    try:
        if text is not None:
            put_file(settings, username, path, (text + "\n").encode("utf-8"))
        elif exists:
            delete_file(settings, username, path)
    except NotAFile as exc:
        # A folder swapped in after the check.
        raise NotAPlainFile(path) from exc
    except NotFound:
        # Removed meanwhile, which is what was asked for; a PUT whose folder
        # vanished is a missing folder.
        if text is not None:
            raise


def _plain_readme_exists(
    settings: Settings, username: str, folder: str, path: str
) -> bool:
    # The folder listing leaves out whatever is neither a file nor a folder,
    # so a README.md that is not listed but answers on its own is one of
    # those. (A symlink answers neither way; see the module docstring.)
    for entry in list_path(settings, username, folder).entries:
        if entry.name == README_NAME:
            if entry.isdir:
                raise NotAPlainFile(path)
            return True
    try:
        list_path(settings, username, path)
    except NotFound:
        return False
    raise NotAPlainFile(path)


def change_description(
    settings: Settings,
    username: str,
    folder: str,
    text: str | None,
    *,
    what: str,
    missing: APIError,
) -> None:
    """
    `write_description` for a `PUT .../description`, with its refusals as API
    errors. `what` names the project or session in messages, as in
    "project 'x'"; `missing` is the 404 for a folder that is gone.
    """
    try:
        write_description(settings, username, folder, text)
    except NotFound as exc:
        raise missing from exc
    except NoAccess as exc:
        raise APIError(
            403,
            "no_access",
            f"Only the person who created README.md in {what} can change it.",
        ) from exc
    except NotAPlainFile as exc:
        raise APIError(
            400,
            "description_rejected",
            f"README.md in {what} is not a plain file (it is a folder or a FIFO), "
            "so it is left alone.",
        ) from exc


def describe_new(
    settings: Settings, username: str, folder: str, text: str, *, what: str
) -> None:
    """
    `write_description` right after a create. The folder exists by then, so
    any failure leaves it without a description, and the answer says so.
    """
    try:
        write_description(settings, username, folder, text)
    except (APIError, NotFound, NoAccess, NotAPlainFile) as exc:
        raise APIError(
            502,
            "description_not_saved",
            f"The {what} was created, but its description could not be saved. "
            "Please add it again on the project page.",
        ) from exc
