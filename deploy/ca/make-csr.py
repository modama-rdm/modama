#!/usr/bin/env python
"""
For internal services (like the mgmt interface), we can't use letsencrypt, so
we have to generate CSRs etc. Needs tomllib for config (py311+).
"""

import subprocess
from pathlib import Path

import tomllib


def make_keys(privkey_path: Path, size: int):
    print(f"generating new private key at {privkey_path}")
    subprocess.check_call(["openssl", "genrsa", "-out", str(privkey_path), str(size)])


def extract_pubkey(privkey_path: Path, pubkey_path: Path):
    print(f"writing public key to {pubkey_path}")
    subprocess.check_call(
        [
            "openssl",
            "rsa",
            "-in",
            str(privkey_path),
            "-pubout",
            "-out",
            str(pubkey_path),
        ]
    )


def make_csr(dest_path: Path, privkey_path: Path, config: dict):
    print(f"generating CSR at {dest_path}")
    subprocess.check_call(
        [
            "openssl",
            "req",
            "-new",
            "-key",
            str(privkey_path),
            "-out",
            str(dest_path),
            "-subj",
            f"/{config['subj']}/CN={config['domain']}",
        ]
    )
    print(
        subprocess.check_output(
            [
                "openssl",
                "req",
                "-text",
                "-in",
                str(dest_path),
                "-noout",
                "-verify",
            ]
        ).decode("utf8")
    )


def main():
    # 0) load certificates from config
    with open("certs.toml", "rb") as f:
        config = tomllib.load(f)

    # 0.1) create certs directory:
    certs = Path("./certs/")
    certs.mkdir(exist_ok=True, mode=0o700)
    if not certs.is_dir():
        raise RuntimeError(f"{certs} is not a directory")

    for cert in config["cert"]:
        # 1) make private key if it doesn't exist
        private_key_path = certs / Path(f"{cert['domain']}.privkey.pem")
        if not private_key_path.exists():
            make_keys(private_key_path, cert.get("key_size", 2048))

        # 2) extract public key
        pubkey_path = certs / Path(f"{cert['domain']}.pubkey.pem")
        if not pubkey_path.exists():
            extract_pubkey(private_key_path, pubkey_path)

        # 3) generate CSR
        csr_dest_path = certs / Path(f"{cert['domain']}.csr.pem")
        make_csr(csr_dest_path, private_key_path, cert)


if __name__ == "__main__":
    main()
