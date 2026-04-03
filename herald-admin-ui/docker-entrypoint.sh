#!/bin/sh
set -e

# Substitute HERALD_URL into nginx config
envsubst '${HERALD_URL}' < /etc/nginx/templates/default.conf.template > /etc/nginx/conf.d/default.conf

exec nginx -g 'daemon off;'
