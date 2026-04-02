#!/bin/sh
set -e

# Fix ownership of /data when mounted as a volume (e.g., Railway, Docker)
# Volume mounts override build-time chown, so we fix it at runtime.
if [ "$(id -u)" = "0" ]; then
    chown -R herald:herald /data 2>/dev/null || true
    exec setpriv --reuid=herald --regid=herald --init-groups "$@"
fi

exec "$@"
