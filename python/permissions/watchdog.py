#!/usr/bin/env python3
import os
import stat
import subprocess
import sys

# This requires the branch
# https://github.com/uellue/PyInotify/tree/nofollowsymlinks
import inotify.adapters
import inotify.constants

if __name__ == "__main__":
    if len(sys.argv) == 1:
        root = "/data"
    elif len(sys.argv) == 2:
        root = sys.argv[1]
    else:
        raise RuntimeError(f"Unrecognized command line arguments {sys.argv}")
    print(f"Creating watch for {root}...")
    i = inotify.adapters.InotifyTree(
        root,
        mask=inotify.constants.IN_CREATE | inotify.constants.IN_MOVED_TO,
        follow_symlinks=False,
    )
    print("Watch created, waiting for events")
    sys.stdout.flush()
    sharing_dir = os.path.join(root, "sharing")
    for event in i.event_gen(yield_nones=False, ignore_missing_new_folders=True):
        (header, type_names, path, filename) = event
        if not (
            (header.mask & inotify.constants.IN_CREATE)
            or (header.mask & inotify.constants.IN_MOVED_TO)
        ):
            continue
        name = os.path.realpath(os.path.join(path, filename))
        if not name.startswith(root):
            print(f"WARNING received event for path {name} outside {root}, ignoring")
            continue
        if name.startswith(sharing_dir):
            continue
        try:
            if "IN_ISDIR" in type_names:
                print("chmod +t", name)
                mode = os.stat(name).st_mode
                os.chmod(name, mode | stat.S_ISVTX)
            else:
                print("remove acls", name)
                # no easy way to cleanly remove ACLs with posix1e
                # since just setting an empty ACL messes up the
                # permission bits
                subprocess.run(
                    ["setfacl", "--remove-all", "--remove-default", name], check=True
                )
                print("chmod g-w", name)
                mode = os.stat(name).st_mode
                os.chmod(name, mode & ~stat.S_IWGRP)
        except Exception as e:  # noqa: BLE001 - must keep watching even if one event fails
            print(e)
        sys.stdout.flush()
