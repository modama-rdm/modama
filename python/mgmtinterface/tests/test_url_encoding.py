import os
from urllib.parse import parse_qs, urlparse

import pytest

BASE_DIR = os.path.join(
    os.path.normpath(os.path.dirname(__file__)),
    "..",
)

os.environ["MGMTINTERFACE_CONFIG"] = "default_config.py"
os.environ["MGMTINTERFACE_SECRET"] = os.path.join(BASE_DIR, "tests/secrets_for_test.py")

from mgmtinterface import (
    add_path_to_query,
    elabftw_erc3_search,
    quote_path,
    sampledb_erc3_search,
    sampledbsearch,
)


@pytest.mark.parametrize(
    "in_path,expected",
    (
        # simple test case for single quote:
        ("/stuff'", "/stuff'"),
        # all the "safe" chars are kept:
        ("/stuff/!$'()*,/:;?@", "/stuff/!$'()*,/:;?@"),
        # importantly, # is percent-encoded:
        ("/stuff/#hash", "/stuff/%23hash"),
        # we use plus-encoding:
        ("/spaces are double plus good/", "/spaces+are+double+plus+good/"),
    ),
)
def test_urlencode_path(in_path, expected):
    assert quote_path(in_path) == expected


# NOTE: the URL (in which we search using only the path) is saved in SampleDB
# in urlencoded form, "as entered". So our search string also needs to search
# for the encoded form of the path.
# The outer encoding is not important to be consistent over time, as that is
# only ephemeral and directly decoded in SampleDB when receiving the request
# (more correct: decoded by flask/werkzeug)
@pytest.mark.parametrize(
    "in_path,expected,decoded",
    (
        (
            "",
            "https://erc-samples.fz-juelich.de/objects/?q=%22%22+in+file_name&advanced=on",
            '"" in file_name',
        ),
        (
            '""',  # why would you call your file ""? what's wrong with you?
            "https://erc-samples.fz-juelich.de/objects/?q=%22%2522%2522%22+in+file_name&advanced=on",
            '"%22%22" in file_name',
        ),
        (
            "/adhoc/data/my stuff/",  # space -> + -> 2B
            "https://erc-samples.fz-juelich.de/objects/?q=%22%2Fadhoc%2Fdata%2Fmy%2Bstuff%2F%22+in+file_name&advanced=on",
            '"/adhoc/data/my+stuff/" in file_name',
        ),
        (
            "/adhoc/data/my'stuff/",  # ' -> 0x27
            "https://erc-samples.fz-juelich.de/objects/?q=%22%2Fadhoc%2Fdata%2Fmy%27stuff%2F%22+in+file_name&advanced=on",
            '"/adhoc/data/my\'stuff/" in file_name',
        ),
        (
            "/adhoc/data/my@stuff/",  # @ is passed through the first url encode, but encoded in the second
            "https://erc-samples.fz-juelich.de/objects/?q=%22%2Fadhoc%2Fdata%2Fmy%40stuff%2F%22+in+file_name&advanced=on",
            '"/adhoc/data/my@stuff/" in file_name',
        ),
        (
            "/weird/!$'()*,/:;?@/",  # all these are "passed through" the inner encoding layer
            "https://erc-samples.fz-juelich.de/objects/?q=%22%2Fweird%2F%21%24%27%28%29%2A%2C%2F%3A%3B%3F%40%2F%22+in+file_name&advanced=on",
            '"/weird/!$\'()*,/:;?@/" in file_name',
        ),
        (
            "/weird/\\/",  # backslash is a potential trouble maker
            "https://erc-samples.fz-juelich.de/objects/?q=%22%2Fweird%2F%255C%2F%22+in+file_name&advanced=on",
            '"/weird/%5C/" in file_name',
        ),
    ),
)
def test_sampledb_search_url(in_path, expected, decoded):
    assert sampledbsearch(in_path) == expected
    parsed = urlparse(sampledbsearch(in_path))
    assert parse_qs(parsed.query)["q"][0] == decoded


# Same double-encoding contract as sampledbsearch, just against
# the ER-C-3 SampleDB instance.
@pytest.mark.parametrize(
    "in_path,expected,decoded",
    (
        (
            "",
            "https://erc-3-samples.fz-juelich.de/objects/?q=%22%22+in+file_name&advanced=on",
            '"" in file_name',
        ),
        (
            '""',
            "https://erc-3-samples.fz-juelich.de/objects/?q=%22%2522%2522%22+in+file_name&advanced=on",
            '"%22%22" in file_name',
        ),
        (
            "/adhoc/data/my stuff/",  # space -> + -> 2B
            "https://erc-3-samples.fz-juelich.de/objects/?q=%22%2Fadhoc%2Fdata%2Fmy%2Bstuff%2F%22+in+file_name&advanced=on",
            '"/adhoc/data/my+stuff/" in file_name',
        ),
        (
            "/adhoc/data/my'stuff/",  # ' -> 0x27
            "https://erc-3-samples.fz-juelich.de/objects/?q=%22%2Fadhoc%2Fdata%2Fmy%27stuff%2F%22+in+file_name&advanced=on",
            '"/adhoc/data/my\'stuff/" in file_name',
        ),
        (
            "/adhoc/data/my@stuff/",  # @ is passed through the first url encode, but encoded in the second
            "https://erc-3-samples.fz-juelich.de/objects/?q=%22%2Fadhoc%2Fdata%2Fmy%40stuff%2F%22+in+file_name&advanced=on",
            '"/adhoc/data/my@stuff/" in file_name',
        ),
        (
            "/weird/!$'()*,/:;?@/",  # all these are "passed through" the inner encoding layer
            "https://erc-3-samples.fz-juelich.de/objects/?q=%22%2Fweird%2F%21%24%27%28%29%2A%2C%2F%3A%3B%3F%40%2F%22+in+file_name&advanced=on",
            '"/weird/!$\'()*,/:;?@/" in file_name',
        ),
        (
            "/weird/\\/",  # backslash is a potential trouble maker
            "https://erc-3-samples.fz-juelich.de/objects/?q=%22%2Fweird%2F%255C%2F%22+in+file_name&advanced=on",
            '"/weird/%5C/" in file_name',
        ),
    ),
)
def test_sampledb_erc3_search_url(in_path, expected, decoded):
    assert sampledb_erc3_search(in_path) == expected
    parsed = urlparse(sampledb_erc3_search(in_path))
    assert parse_qs(parsed.query)["q"][0] == decoded


# ElabFTW gets the quote_path-encoded path as the `q` search term on the
# experiments listing, double-encoded like the SampleDB searches: the decoded
# `q` must still be the encoded form of the path, as ElabFTW stores URLs
# "as entered".
@pytest.mark.parametrize(
    "in_path,expected,decoded",
    (
        (
            "",
            "https://erc3elab.app.fz-juelich.de/experiments.php?q=",
            "",
        ),
        (
            "/adhoc/datamanagement/chaitali",  # nothing to escape -> single-encoded
            "https://erc3elab.app.fz-juelich.de/experiments.php?q=%2Fadhoc%2Fdatamanagement%2Fchaitali",
            "/adhoc/datamanagement/chaitali",
        ),
        (
            "/adhoc/data/my stuff/",  # space -> + -> 2B
            "https://erc3elab.app.fz-juelich.de/experiments.php?q=%2Fadhoc%2Fdata%2Fmy%2Bstuff%2F",
            "/adhoc/data/my+stuff/",
        ),
        (
            "/adhoc/data/my'stuff/",  # ' -> 0x27
            "https://erc3elab.app.fz-juelich.de/experiments.php?q=%2Fadhoc%2Fdata%2Fmy%27stuff%2F",
            "/adhoc/data/my'stuff/",
        ),
        (
            "/weird/!$'()*,/:;?@/",  # all these are "passed through" the inner encoding layer
            "https://erc3elab.app.fz-juelich.de/experiments.php?q=%2Fweird%2F%21%24%27%28%29%2A%2C%2F%3A%3B%3F%40%2F",
            "/weird/!$'()*,/:;?@/",
        ),
        (
            "/weird/\\/",  # backslash is a potential trouble maker
            "https://erc3elab.app.fz-juelich.de/experiments.php?q=%2Fweird%2F%255C%2F",
            "/weird/%5C/",
        ),
    ),
)
def test_elabftw_erc3_search_url(in_path, expected, decoded):
    assert elabftw_erc3_search(in_path) == expected
    parsed = urlparse(elabftw_erc3_search(in_path))
    assert parse_qs(parsed.query, keep_blank_values=True)["q"][0] == decoded


def test_add_path_to_query():
    assert (
        add_path_to_query("http://localhost/browse/", "/data/prime'ä")
        == "http://localhost/browse/?path=/data/prime'%C3%A4"
    )
