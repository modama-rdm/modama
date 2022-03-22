import os
import stat
import subprocess
import sys

import inotify.adapters
import inotify.constants


if __name__ == '__main__':
    print("Creating watch for /data...")
    i = inotify.adapters.InotifyTree(
        '/data',
        mask=inotify.constants.IN_CREATE
    )
    print("Watch created, waiting for events")
    sys.stdout.flush()
    for event in i.event_gen(yield_nones=False):
        (header, type_names, path, filename) = event
        if path.startswith('/data/sharing'):
            continue
        name = os.path.join(path, filename)
        try:
            mode = os.stat(name).st_mode
            if 'IN_ISDIR' in type_names:
                print("chmod +t", name)
                os.chmod(name, mode | stat.S_ISVTX)
            else:
                print("remove acls", name)
                # no easy way to cleanly remove ACLs with posix1e
                # since just setting an empty ACL messes up the
                # permission bits
                subprocess.run(
                    ["setfacl", "--remove-all", "--remove-default", name],
                    check=True
                )
                print("chmod g-w", name)
                os.chmod(name, mode & ~stat.S_IWGRP)
        except Exception as e:
            print(e)
        sys.stdout.flush()

