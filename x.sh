#!/usr/bin/env sh
set -x
make -C build
mkdir -p dist
cp build/guppyctl dist/
