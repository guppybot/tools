#!/usr/bin/env sh
set -eux

#yum -y update
#yum -y install ca-certificates
cp -r /checkout /work
cp /task /run_task.sh
chmod +x /run_task.sh
cd /work
exec /run_task.sh
