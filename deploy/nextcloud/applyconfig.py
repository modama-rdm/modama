#!/usr/bin/env python3

import json
import subprocess
import sys

OCC = ("sudo", "-u", "www-data", "php", "-d", "memory_limit=-1", "/var/www/html/occ")


def occ(*args, occ_quiet=False, may_fail=False, **kwargs):
    """Run occ as www-data. With may_fail, a failure is expected: it returns
    None instead of raising, and the output goes to stdout like any other."""
    command = OCC + args
    if not occ_quiet:
        print(command)
    else:
        print("printing command and output suppressed")
    try:
        result = subprocess.run(
            command, capture_output=True, check=True, text=True, **kwargs
        )
        if not occ_quiet:
            print(result.stdout)
        return result
    except subprocess.CalledProcessError as e:
        if not occ_quiet:
            # stderr: uptest.sh discards stdout, but an error has to be seen
            print(
                e.returncode,
                e.stdout,
                e.stderr,
                file=sys.stdout if may_fail else sys.stderr,
            )
        if may_fail:
            return None
        raise


if __name__ == "__main__":
    occ("user:disable", "root")

    for app in (
        "federation",
        "files_external",
        "files_sharing",
        "files_trashbin",
        "files_versions",
        "theming",
        "user_ldap",
    ):
        # Make sure database is initialized for
        # the app in question
        occ("app:disable", app, may_fail=True)
        occ("app:enable", app)

    for app in ("weather_status", "user_status", "survey_client", "firstrunwizard"):
        occ("app:disable", app, may_fail=True)

    # Disable skeleton and template: Only external storage in user folder
    occ("config:system:set", "skeletondirectory")
    occ("config:system:set", "templatedirectory")

    # Show files after logging in
    occ("config:system:set", "defaultapp", '--value="files"')

    # Properly initialize LDAP config
    occ("ldap:delete", "s01", may_fail=True)
    occ("ldap:create-empty-config")

    occ("config:import", "/nextcloud-config.json")
    occ("config:import", "/nextcloud-site-config.json")
    # pipe for www-data read permission
    # Passwords, salts and such
    with open("/nextcloud-config-secret.json", "r", encoding="UTF-8") as f:
        occ("config:import", stdin=f)

    # Apply URL rewrite for subfolder
    occ("maintenance:update:htaccess")

    externals_output = occ("files_external:list", "--output=json")
    externals = json.loads(externals_output.stdout)

    for external in externals:
        occ("files_external:delete", "--yes", str(external["mount_id"]))

    occ("files_external:import", "/externalstorage.json")

    externals_output = occ("files_external:list", "--output=json")
    externals = json.loads(externals_output.stdout)

    if len(externals) != 1:
        raise RuntimeError("Number of external storages is not 1!")

    with open("/externalstorage_pw", "r", encoding="UTF-8") as f:
        pw = f.read()
        occ(
            "files_external:config",
            str(externals[0]["mount_id"]),
            "password",
            pw,
            occ_quiet=True,
        )

    occ("theming:config", "logo", "/Icon-Nextcloud.svg")
    occ("theming:config", "logoheader", "/Icon-Nextcloud.svg")
    occ("theming:config", "favicon", "/favicon.jpg")

    subprocess.run(
        ("service", "apache2", "reload"), capture_output=True, check=True, text=True
    )
