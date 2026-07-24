#!/usr/bin/env python3
import os
import signal
import subprocess
import sys

from supervisor import childutils


def main():
    while True:
        headers, _payload = childutils.listener.wait()
        childutils.listener.ok()
        if headers["eventname"].startswith("TICK"):
            print("running renew-cert.sh...", file=sys.stderr)
            subprocess.run(["/usr/bin/renew-cert.sh"], stdout=sys.stderr, check=False)
            print("renew-cert.sh done", file=sys.stderr)
        elif headers["eventname"] == "PROCESS_STATE_FATAL":
            os.kill(os.getppid(), signal.SIGTERM)
        else:
            print(f"unknown event: {headers['eventname']}", file=sys.stderr)


if __name__ == "__main__":
    main()
