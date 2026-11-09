# `modama-agent` - getting data into `modama`

To ingest data into `modama`, we provide a small software called `modama-agent`.
Using `modama-agent`, you can create a networked session folder, which we call
an "ingest share". 

## How to create an ingest share

- Step 1: start the `modama-agent` GUI

![Connecting to modama-agent](images/agent/connecting.png)

![The modama-agent ingest page while logged out](images/agent/overview-logged-out.png)

- Step 2: log in

![The modama-agent login screen](images/agent/login.png)

- Step 3: click "Create ingest share" button

TODO: describe how to choose a project.

![Selecting a project for the ingest share](images/agent/project-selection.png)

- Step 4: enter session details or select existing session

TODO: describe the choice between a new and an existing session.

![Selecting an existing session](images/agent/session-selection.png)

![Creating a new session](images/agent/create-session.png)

- Step 5: confirm

TODO: describe the confirmation step.

Now the ingest share should be available, either as a mapped network drive,
or as a mounted network filesystem under Linux.

![The ingest page with a foreground ingest](images/agent/active-session.png)

## How to upload data to the ingest share

The ingest share should automatically open on all data-producing systems. On
Windows, it will be mapped as a network drive (for example `Z:\`). Simply save
your data on this network drive, or copy/drag is there, and it will be stored
in the data management.

## Automation: how to get the current session folder?

If you would like to integrate data management into your automation, for example
into an acquisition script or notebook, this is easily possible! `modama-agent`
provides a local HTTP API, which you can query for the current
session folder. We call this the "foreground ingest". The GUI has ready-made
Python code for this: on the ingest page, click "Use in Python" below the mount
path. The code looks like this:

```python
# The session folder for new data (the foreground ingest),
# from the modama agent on this computer. Run this before you
# start saving.
import json
import pathlib
import urllib.request

url = "http://localhost:3003/api/shares/foreground/"
headers = {"X-API-Version": "2.0.0"}
request = urllib.request.Request(url, headers=headers)
with urllib.request.urlopen(request, timeout=10) as response:
    share = json.load(response)
if "mount_path" not in share:
    reason = share.get("message", "No foreground ingest.")
    raise RuntimeError(reason)
session_folder = share["mount_path"]
raw_folder = pathlib.Path(session_folder) / "raw"
```

The `session_folder` is valid only on the computer where you ran this snippet,
different computers have their own mount paths.
