#!/bin/sh
set -e

PORT="${PORT:-8080}"
HERALD_URL="${HERALD_URL:-http://localhost:6201}"

# Ensure URL has a scheme
case "$HERALD_URL" in
  http://*|https://*) ;;
  *) HERALD_URL="https://${HERALD_URL}" ;;
esac

# Strip trailing slash
HERALD_URL="${HERALD_URL%/}"

# Inject Herald URL into the frontend at runtime
cat > /app/dist/config.js << EOF
window.__HERALD_URL__ = "${HERALD_URL}";
EOF

exec npx serve dist -l "$PORT" -s
