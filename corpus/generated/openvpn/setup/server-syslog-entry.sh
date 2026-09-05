#!/bin/bash
# Container PID 1 for the syslog phase: rsyslogd first (so nothing is lost),
# then the real openvpn in the foreground with --syslog.
set -e
mkdir -p /var/spool/rsyslog
rsyslogd -n -f /etc/rsyslog.conf &
sleep 1
exec openvpn --config /etc/openvpn/server.conf
