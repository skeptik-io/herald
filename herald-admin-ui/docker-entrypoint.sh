#!/bin/sh
set -e

PORT="${PORT:-8080}"
HERALD_URL="${HERALD_URL:-http://localhost:6201}"

# Inject Herald URL into the frontend at runtime
cat > /app/dist/config.js << EOF
window.__HERALD_URL__ = "${HERALD_URL}";
EOF

exec npx serve dist -l "$PORT" -s
