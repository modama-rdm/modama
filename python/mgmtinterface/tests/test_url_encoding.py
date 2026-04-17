import os
from urllib.parse import urlparse, parse_qs

BASE_DIR = os.path.join(
    os.path.normpath(os.path.dirname(__file__)),
    "..",
)

os.environ["MGMTINTERFACE_CONFIG"] = "default_config.py"
os.environ["MGMTINTERFACE_SECRET"] = os.path.join(BASE_DIR, "tests/secrets_for_test.py")

from mgmtinterface import sampledbsearch, quote_path, add_path_to_query
import pytest


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


def test_add_path_to_query():
    assert (
        add_path_to_query("http://localhost/browse/", "/data/prime'ä")
        == "http://localhost/browse/?path=/data/prime'%C3%A4"
    )
