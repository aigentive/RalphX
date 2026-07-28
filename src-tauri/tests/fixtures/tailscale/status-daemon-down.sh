#!/bin/sh
printf '%s' 'failed to connect to local tailscaled; it does not appear to be running.' >&2
exit 1
