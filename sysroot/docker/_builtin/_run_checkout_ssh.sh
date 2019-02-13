#!/usr/bin/env sh
set -eu

mkdir -p /root/.ssh
chmod 700 /root/.ssh
echo 'Host *' > /root/.ssh/config
echo '  LogLevel FATAL' >> /root/.ssh/config
echo '  StrictHostKeyChecking no' >> /root/.ssh/config
cp /secrets/ssh_key /root/.ssh/id_rsa
chmod 600 /root/.ssh/id_rsa

git clone -q --recursive ${GUPPY_GIT_REMOTE_URL} /checkout
