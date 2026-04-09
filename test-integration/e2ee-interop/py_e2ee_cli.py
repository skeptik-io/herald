#!/usr/bin/env python3
"""Small CLI for E2EE cross-SDK interop testing.

Usage:
    python py_e2ee_cli.py encrypt <hex_shared_secret> <version> <plaintext> <stream_id>
    python py_e2ee_cli.py decrypt <hex_shared_secret> <version> <ciphertext> <stream_id>
    python py_e2ee_cli.py blind   <hex_shared_secret> <plaintext>
"""
import json
import sys
import os

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "..", "herald-admin-python"))
from herald_admin.e2ee import create_session, decode_blind_tokens


def main():
    cmd = sys.argv[1]

    if cmd == "encrypt":
        secret = bytes.fromhex(sys.argv[2])
        version = int(sys.argv[3])
        plaintext = sys.argv[4]
        stream_id = sys.argv[5]
        session = create_session(secret, version)
        ct = session.encrypt_convergent(plaintext, stream_id)
        print(json.dumps({"ciphertext": ct}))

    elif cmd == "decrypt":
        secret = bytes.fromhex(sys.argv[2])
        version = int(sys.argv[3])
        ciphertext = sys.argv[4]
        stream_id = sys.argv[5]
        session = create_session(secret, version)
        pt = session.decrypt(ciphertext, stream_id)
        print(json.dumps({"plaintext": pt}))

    elif cmd == "blind":
        secret = bytes.fromhex(sys.argv[2])
        plaintext = sys.argv[3]
        session = create_session(secret, 1)
        wire = session.blind(plaintext)
        print(json.dumps({"wire": wire}))

    else:
        print(f"unknown command: {cmd}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
