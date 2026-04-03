#!/bin/sh
set -e

# Substitute HERALD_URL into nginx config
export PORT="${PORT:-8080}"
envsubst '${HERALD_URL} ${PORT}' < /etc/nginx/templates/default.conf.template > /etc/nginx/conf.d/default.conf

exec nginx -g 'daemon off;'
