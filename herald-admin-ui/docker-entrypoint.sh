#!/bin/sh
set -e

PORT="${PORT:-8080}"
HERALD_URL="${HERALD_URL:-http://localhost:6201}"

sed \
  -e "s|__PORT__|${PORT}|g" \
  -e "s|__HERALD_URL__|${HERALD_URL}|g" \
  /etc/nginx/templates/default.conf.template > /etc/nginx/conf.d/default.conf

exec nginx -g 'daemon off;'
