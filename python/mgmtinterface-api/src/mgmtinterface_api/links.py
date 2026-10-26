"""
Where a browsed path opens elsewhere: the links and transfer commands the
browse endpoint answers with.

A port of the Flask app's template filters (`nextcloudlink`, `sampledbsearch`
and the others). The path is encoded in three different ways on purpose, each
the way its target expects it. The SampleDB and ElabFTW searches look for the
browse URL people pasted into their entries, which those systems store "as
entered", so `quote_path` has to stay byte for byte what the legacy UI
produced; `tests/test_links.py` carries the cases of the Flask app's
`test_url_encoding.py` for that reason.
"""

import posixpath
import shlex
from urllib.parse import quote, quote_plus

from .config import Settings


def quote_path(path: str) -> str:
    """
    The path as it appears in the query part of a browse URL, as close to the
    WHATWG URL standard as `quote_plus` gets: `/` stays, a space becomes `+`,
    and `#` becomes `%23`.
    """
    return quote_plus(path, safe="!$'()*,/:;?@")


def nextcloud_link(settings: Settings, path: str, isdir: bool) -> str:
    # Nextcloud opens folders; for a file, its folder.
    if not isdir:
        path = posixpath.dirname(path)
    return settings.nextcloud_base + quote_plus(path)


def jupyter_link(settings: Settings, path: str, isdir: bool) -> str:
    if not isdir:
        path = posixpath.dirname(path)
    return settings.jupyter_base + quote(path)


def libertem_link(settings: Settings, path: str) -> str:
    return settings.libertem_base + quote_plus(path)


def image_viewer_link(settings: Settings, path: str) -> str:
    return settings.image_viewer_base + quote_plus(path)


def sampledb_search(settings: Settings, path: str) -> str:
    return _sampledb_search(settings.sampledb_base, path)


def sampledb_erc3_search(settings: Settings, path: str) -> str:
    return _sampledb_search(settings.sampledb_erc3_base, path)


def _sampledb_search(base: str, path: str) -> str:
    # The search term is the encoded path, because that is what SampleDB
    # stored; the term is then encoded once more as a query value.
    search = f'"{quote_path(path)}" in file_name'
    return f"{base}objects/?q={quote_plus(search)}&advanced=on"


def elabftw_erc3_search(settings: Settings, path: str) -> str:
    return (
        f"{settings.elabftw_erc3_base}experiments.php?q={quote_plus(quote_path(path))}"
    )


def rsync_command(settings: Settings, path: str, isdir: bool, user: str) -> str:
    target = settings.moellenstedt_base + path
    if isdir:
        target += "/"
        source = "<src_path>/"
    else:
        source = "<src_file>"
    return (
        'rsync -av --info=progress2 -e "ssh -T -c aes128-ctr -o Compression=no -x" '
        f'"{source}" {shlex.quote(user + "@" + target)}'
    )


def sftp_command(settings: Settings, path: str, isdir: bool, user: str) -> str:
    if not isdir:
        path = posixpath.dirname(path)
    target = settings.moellenstedt_base + path
    return f"sftp -c aes128-ctr -o Compression=no {shlex.quote(user + '@' + target)}"


def smb_windows_path(settings: Settings, path: str) -> str:
    return settings.main_smb_base + path.replace("/", "\\")


def smb_unix_url(settings: Settings, path: str) -> str:
    return settings.smb_url_base + quote(path)


def entry_access(settings: Settings, path: str, isdir: bool) -> dict[str, str]:
    """
    The links for one entry of a listing, the columns the legacy table had:
    Nextcloud and JupyterHub for a folder, the image viewer for a file,
    LiberTEM and the three searches for both.
    """
    if isdir:
        access = {
            "nextcloud": nextcloud_link(settings, path, isdir),
            "jupyter": jupyter_link(settings, path, isdir),
        }
    else:
        access = {"imageviewer": image_viewer_link(settings, path)}
    access["libertem"] = libertem_link(settings, path)
    access["sampledb"] = sampledb_search(settings, path)
    access["sampledb_erc3"] = sampledb_erc3_search(settings, path)
    access["elabftw_erc3"] = elabftw_erc3_search(settings, path)
    return access


def location_access(
    settings: Settings, path: str, isdir: bool, user: str
) -> dict[str, str]:
    """
    The links and transfer commands for the browsed path itself, as the
    legacy page's "Access this file or folder" cards had them. Unlike an
    entry, a file gets the Nextcloud and JupyterHub links of its folder too.
    """
    access = {
        "nextcloud": nextcloud_link(settings, path, isdir),
        "jupyter": jupyter_link(settings, path, isdir),
        "libertem": libertem_link(settings, path),
        "sampledb": sampledb_search(settings, path),
        "sampledb_erc3": sampledb_erc3_search(settings, path),
        "elabftw_erc3": elabftw_erc3_search(settings, path),
        "rsync": rsync_command(settings, path, isdir, user),
        "sftp": sftp_command(settings, path, isdir, user),
        "smb_windows": smb_windows_path(settings, path),
        "smb_unix": smb_unix_url(settings, path),
    }
    if not isdir:
        access["imageviewer"] = image_viewer_link(settings, path)
    return access
