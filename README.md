# Guppybot: tools

This repository contains tools used for automating local machines with GPUs.

Currently there are two executable tools:

* guppybot: a system daemon for automatically running tasks
* guppyctl: a frontend tool that can be used standalone or with the daemon

## Installation

1.  Build with `make`.
2.  Do `sudo make install` to install the frontend to `/usr/local/bin/guppyctl`.
3.  Run `guppyctl install-self` to finish the installation.

## Configuration

1.  Required: edit `/etc/guppybot/api` with your API authentication details
    (API ID + secret token).
2.  Optional: `/etc/guppybot/machine` will be automatically filled with a
    working default config, edit this if desired.

## License

Licensed under either the MIT license or the Apache 2.0 license at your option.
