"""
The browse links, against the production bases the settings default to.

The SampleDB and ElabFTW cases are those of the Flask app's
`tests/test_url_encoding.py`: the searches look for browse URLs that people
pasted into their entries, which those systems store "as entered", so the
encoding must not drift from what the legacy UI produced.
"""

from urllib.parse import parse_qs, urlparse

import pytest
from mgmtinterface_api.config import Settings
from mgmtinterface_api.links import (
    elabftw_erc3_search,
    entry_access,
    image_viewer_link,
    jupyter_link,
    libertem_link,
    location_access,
    nextcloud_link,
    quote_path,
    rsync_command,
    sampledb_erc3_search,
    sampledb_search,
    sftp_command,
    smb_unix_url,
    smb_windows_path,
)


@pytest.mark.parametrize(
    "in_path,expected",
    (
        ("/stuff'", "/stuff'"),
        # All the "safe" characters are kept.
        ("/stuff/!$'()*,/:;?@", "/stuff/!$'()*,/:;?@"),
        # Importantly, # is percent-encoded.
        ("/stuff/#hash", "/stuff/%23hash"),
        # Plus-encoding for spaces.
        ("/spaces are double plus good/", "/spaces+are+double+plus+good/"),
    ),
)
def test_quote_path(in_path: str, expected: str):
    assert quote_path(in_path) == expected


# The URL is stored in SampleDB in urlencoded form, "as entered", so the search
# term is the encoded path. The outer encoding is only ephemeral: SampleDB
# decodes it on arrival.
SAMPLEDB_CASES = (
    (
        "",
        "objects/?q=%22%22+in+file_name&advanced=on",
        '"" in file_name',
    ),
    (
        '""',
        "objects/?q=%22%2522%2522%22+in+file_name&advanced=on",
        '"%22%22" in file_name',
    ),
    (
        "/adhoc/data/my stuff/",  # space -> + -> 2B
        "objects/?q=%22%2Fadhoc%2Fdata%2Fmy%2Bstuff%2F%22+in+file_name&advanced=on",
        '"/adhoc/data/my+stuff/" in file_name',
    ),
    (
        "/adhoc/data/my'stuff/",  # ' -> 0x27
        "objects/?q=%22%2Fadhoc%2Fdata%2Fmy%27stuff%2F%22+in+file_name&advanced=on",
        '"/adhoc/data/my\'stuff/" in file_name',
    ),
    (
        "/adhoc/data/my@stuff/",  # @ passes the inner encoding, not the outer
        "objects/?q=%22%2Fadhoc%2Fdata%2Fmy%40stuff%2F%22+in+file_name&advanced=on",
        '"/adhoc/data/my@stuff/" in file_name',
    ),
    (
        "/weird/!$'()*,/:;?@/",  # all passed through the inner layer
        "objects/?q=%22%2Fweird%2F%21%24%27%28%29%2A%2C%2F%3A%3B%3F%40%2F%22+in+file_name&advanced=on",
        '"/weird/!$\'()*,/:;?@/" in file_name',
    ),
    (
        "/weird/\\/",  # backslash is a potential trouble maker
        "objects/?q=%22%2Fweird%2F%255C%2F%22+in+file_name&advanced=on",
        '"/weird/%5C/" in file_name',
    ),
)


@pytest.mark.parametrize("in_path,expected,decoded", SAMPLEDB_CASES)
def test_sampledb_search_url(
    settings: Settings, in_path: str, expected: str, decoded: str
):
    url = sampledb_search(settings, in_path)
    assert url == "https://erc-samples.fz-juelich.de/" + expected
    assert parse_qs(urlparse(url).query)["q"][0] == decoded


@pytest.mark.parametrize("in_path,expected,decoded", SAMPLEDB_CASES)
def test_sampledb_erc3_search_url(
    settings: Settings, in_path: str, expected: str, decoded: str
):
    url = sampledb_erc3_search(settings, in_path)
    assert url == "https://erc-3-samples.fz-juelich.de/" + expected
    assert parse_qs(urlparse(url).query)["q"][0] == decoded


# ElabFTW gets the quote_path-encoded path as the `q` search term, double
# encoded like the SampleDB searches.
@pytest.mark.parametrize(
    "in_path,expected,decoded",
    (
        ("", "experiments.php?q=", ""),
        (
            "/adhoc/datamanagement/chaitali",  # nothing to escape -> single-encoded
            "experiments.php?q=%2Fadhoc%2Fdatamanagement%2Fchaitali",
            "/adhoc/datamanagement/chaitali",
        ),
        (
            "/adhoc/data/my stuff/",
            "experiments.php?q=%2Fadhoc%2Fdata%2Fmy%2Bstuff%2F",
            "/adhoc/data/my+stuff/",
        ),
        (
            "/adhoc/data/my'stuff/",
            "experiments.php?q=%2Fadhoc%2Fdata%2Fmy%27stuff%2F",
            "/adhoc/data/my'stuff/",
        ),
        (
            "/weird/!$'()*,/:;?@/",
            "experiments.php?q=%2Fweird%2F%21%24%27%28%29%2A%2C%2F%3A%3B%3F%40%2F",
            "/weird/!$'()*,/:;?@/",
        ),
        (
            "/weird/\\/",
            "experiments.php?q=%2Fweird%2F%255C%2F",
            "/weird/%5C/",
        ),
    ),
)
def test_elabftw_erc3_search_url(
    settings: Settings, in_path: str, expected: str, decoded: str
):
    url = elabftw_erc3_search(settings, in_path)
    assert url == "https://erc3elab.app.fz-juelich.de/" + expected
    assert parse_qs(urlparse(url).query, keep_blank_values=True)["q"][0] == decoded


def test_app_links_encode_the_path_each_in_their_way(settings: Settings):
    path = "/adhoc/proj/my session/raw"
    # Nextcloud and LiberTEM take a plus-encoded path, JupyterHub and the
    # Samba URL a percent-encoded one, the UNC path backslashes.
    assert nextcloud_link(settings, path, isdir=True).endswith(
        "?dir=/ER-C-Data%2Fadhoc%2Fproj%2Fmy+session%2Fraw"
    )
    assert jupyter_link(settings, path, isdir=True).endswith(
        "/lab/tree/er-c-data/adhoc/proj/my%20session/raw"
    )
    assert libertem_link(settings, path).endswith(
        "path=/storage/er-c-data%2Fadhoc%2Fproj%2Fmy+session%2Fraw"
    )
    assert image_viewer_link(settings, path).endswith(
        "?path=/storage/er-c-data%2Fadhoc%2Fproj%2Fmy+session%2Fraw"
    )
    assert smb_windows_path(settings, path) == (
        "\\\\er-c-data-smb.fz-juelich.de\\er-c-data\\adhoc\\proj\\my session\\raw"
    )
    assert smb_unix_url(settings, path) == (
        "smb://er-c-data-smb.fz-juelich.de/er-c-data/adhoc/proj/my%20session/raw"
    )


def test_folder_links_of_a_file_point_at_its_folder(settings: Settings):
    file = "/adhoc/proj/sess/raw/frame_000.tif"
    assert nextcloud_link(settings, file, isdir=False) == nextcloud_link(
        settings, "/adhoc/proj/sess/raw", isdir=True
    )
    assert jupyter_link(settings, file, isdir=False) == jupyter_link(
        settings, "/adhoc/proj/sess/raw", isdir=True
    )


def test_transfer_commands_quote_for_the_shell(settings: Settings):
    folder = "/adhoc/proj/my session"
    assert rsync_command(settings, folder, isdir=True, user="shanker") == (
        'rsync -av --info=progress2 -e "ssh -T -c aes128-ctr -o Compression=no -x" '
        '"<src_path>/" '
        "'shanker@moellenstedt.iff.kfa-juelich.de:/storage/er-c-data/adhoc/proj/my session/'"
    )
    assert rsync_command(settings, folder + "/a.dat", isdir=False, user="u") == (
        'rsync -av --info=progress2 -e "ssh -T -c aes128-ctr -o Compression=no -x" '
        '"<src_file>" '
        "'u@moellenstedt.iff.kfa-juelich.de:/storage/er-c-data/adhoc/proj/my session/a.dat'"
    )
    # sftp opens the folder, also for a file.
    assert sftp_command(settings, folder + "/a.dat", isdir=False, user="u") == (
        "sftp -c aes128-ctr -o Compression=no "
        "'u@moellenstedt.iff.kfa-juelich.de:/storage/er-c-data/adhoc/proj/my session'"
    )
    assert sftp_command(settings, "/adhoc/proj", isdir=True, user="u") == (
        "sftp -c aes128-ctr -o Compression=no "
        "u@moellenstedt.iff.kfa-juelich.de:/storage/er-c-data/adhoc/proj"
    )


def test_entry_access_has_the_columns_of_the_legacy_table(settings: Settings):
    assert set(entry_access(settings, "/adhoc/proj", isdir=True)) == {
        "nextcloud",
        "jupyter",
        "libertem",
        "sampledb",
        "sampledb_erc3",
        "elabftw_erc3",
    }
    assert set(entry_access(settings, "/adhoc/proj/a.dat", isdir=False)) == {
        "imageviewer",
        "libertem",
        "sampledb",
        "sampledb_erc3",
        "elabftw_erc3",
    }


def test_location_access_adds_the_transfer_commands(settings: Settings):
    folder = location_access(settings, "/adhoc/proj", isdir=True, user="u")
    assert set(folder) == {
        "nextcloud",
        "jupyter",
        "libertem",
        "sampledb",
        "sampledb_erc3",
        "elabftw_erc3",
        "rsync",
        "sftp",
        "smb_windows",
        "smb_unix",
    }
    # A file gets the image viewer on top, and keeps its folder's Nextcloud
    # and JupyterHub links, unlike a file in a listing.
    file = location_access(settings, "/adhoc/proj/a.dat", isdir=False, user="u")
    assert set(file) == set(folder) | {"imageviewer"}
