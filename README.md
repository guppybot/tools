# Guppybot: tools

This repository contains tools used for automating GPU machines.

Currently there are two executable tools:

* guppybot: a system daemon for automatically running tasks
* guppyctl: a frontend tool that can be used standalone or with the daemon

## Prerequisites

Building the Guppybot tools requires a recent Rust stable release (1.32.0 or
newer). Install Rust using [rustup](https://rustup.rs/).

Running tasks with Guppybot, either manually (via `guppyctl tmp-run`) or
automatically (via the daemon), requires Docker, as well as nvidia-docker2
for GPU-based runs.

The Guppybot tools are intended for use on Linux systems with systemd.
Non-Linux but unix-like environments _might_ work but are not supported.

## Installation

1.  Build with `make && sudo make install`. This installs the frontend to
    `/usr/local/bin/guppyctl`.
2.  Run `sudo guppyctl self-install` to install necessary files, including the
    daemon which gets installed to `/usr/local/bin/guppybot`.
3.  Run `sudo systemctl daemon-reload` followed by `sudo systemctl start guppybot`
    to start the daemon.

## API configuration

1.  Edit `/etc/guppybot/api` with your API authentication details
    (API ID + secret token).
2.  Next, run `sudo guppyctl auth` to authenticate with the guppybot.org API
    server.
3.  Finally, run `sudo guppyctl register` to register your local machine with
    the registry (https://guppybot.org). Your machine will then be registered
    as an automated worker that is eligible to run tasks. Initially however,
    your machine is not subscribed to any source of tasks, so your machine
    will not run tasks until you specifically subscribe to a task source.

## Optional configuration

* `/etc/guppybot/machine` will be automatically filled with a working default
  config, edit this if desired.
  (Note: If your system configuration changes, or if you modify
  `/etc/guppybot/machine`, just run `sudo guppyctl register` again to refresh
  the registry's view of your local machine.)

## License

Licensed under either the MIT license or the Apache 2.0 license at your option.
