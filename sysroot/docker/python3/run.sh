#!/usr/bin/env sh
set -eux

python3 --version
apt-get update
apt-get install -y --no-install-recommends ca-certificates curl git
cp -r /checkout /work
cp /task /run_task.sh
chmod +x /run_task.sh
cd /work
exec /run_task.sh
