# Guppybot: tools

This repository contains tools used for automating local machines with GPUs.

Currently there are two executable tools:

* guppybot: a system daemon for automatically running tasks
* guppyctl: a frontend tool that can be used standalone or with the daemon

## Installation

The Guppybot tools are intended for use on Linux systems; non-Linux unix-like
environments may work but are not currently supported.
Building the Guppybot tools requires a recent Rust stable release (1.32.0 or
newer). Install Rust using [rustup](https://rustup.rs/).

1.  Build with `make`.
2.  Do `sudo make install` to install the frontend to `/usr/local/bin/guppyctl`.
3.  Run `guppyctl install-self` to finish the installation.

Run `guppyctl -h` to check that the installation worked.

## API configuration

1.  Edit `/etc/guppybot/api` with your API authentication details
    (API ID + secret token).
2.  Next, run `guppyctl auth` to authenticate with the guppybot.org API server.
3.  Finally, run `guppyctl register-machine` to register your local machine as
    an automated worker for running tasks.

## Optional configuration

* `/etc/guppybot/machine` will be automatically filled with a working default
  config, edit this if desired.

## License

Licensed under either the MIT license or the Apache 2.0 license at your option.
