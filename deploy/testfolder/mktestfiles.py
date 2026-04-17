import os

basepath = "data/adhoc/datamanagement/pytest"

# Follow unit tests in python/mgmtinterface/tests/test_url_encoding.py
# but make sure that none of them is a substring of another so that the SampleDB
# search doesn't match any other entry
cases = (
    '""',
    "my stuff/",
    "my'stuff/",
    "my@stuff",
    "!$'()*,/:;?@:-D/",
    "!$'()*,/:;?@\\",
)

os.makedirs(basepath, exist_ok=True)

for case in cases:
    full = os.path.join(basepath, case)
    if case and case[-1] == "/":
        os.makedirs(full, exist_ok=True)
    else:
        with open(full, "w") as f:
            pass
