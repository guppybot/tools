#!/usr/bin/env sh
set -eux

python --version
#apt-get update
#apt-get install -y --no-install-recommends ca-certificates
ln -s /checkout /work
cp /task /run_task.sh
chmod +x /run_task.sh
cd /work
exec /run_task.sh
