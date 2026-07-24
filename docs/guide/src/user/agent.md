# `modama-agent` - getting data into `modama`

To ingest data into `modama`, we provide a small software called `modama-agent`.
Using `modama-agent`, you can create a networked session folder, which we call
an "ingest share". 

## How to create an ingest share

- Step 1: start the `modama-agent` GUI
- Step 2: log in
- Step 3: click "Create ingest share" button
- Step 4: enter session details or select existing session
- Step 5: confirm

TODO: build GUI and insert screenshots here

Now the ingest share should be available, either as a mapped network drive,
or as a mounted network filesystem under Linux.

TODO: insert GUI here that shows currently active ingest shares

## How to upload data to the ingest share

The ingest share should automatically open on all data-producing systems. On
Windows, it will be mapped as a network drive (for example `Z:\`). Simply save
your data on this network drive, or copy/drag is there, and it will be stored
in the data management.

## Automation: how to get the current session folder?

If you would like to integrate data management into your automation, for example
into an acquisition script or notebook, this is easily possible! `modama-agent`
provides a local HTTP API on port 3003, which you can query for the current
session folder. We call this the "foreground share". For example, in Python:

```python
import requests

shares = requests.get("http://localhost:3003/shares/foreground/").json()
# if there is no foreground share: {"type": None}
# if there is a foreground share: {"type": "Share", "dest_path": "...", "mount_path": "Z:\\..."}
```

It is a good idea to request this information at the beginning of the script,
as the foreground share is replaced when the next session starts, but your
script might still want to save data into the correct session.
