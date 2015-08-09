# Wacom Mapping Watcher

This is a little utility that runs in background of X session and ensures that Wacom tablets are mapped to one particular screen. This is useful when using multiple monitors with Wacom tablet/digitizer (by default X.org will stretch the active area of the tablet/digitizer to cover all connected monitors).

## Invocation

    Usage: wacom-output-mapping-watcher [options]

    Options:
        -w --watch          watch for RANDR events and reconfigure Wacom tablets
        -o --output OUTPUT  name of X RANDR output to which Wacom tables will be
                            mapped
        -h --help           print this help menu

To start automatically with X session, use `~/.xprofile` (this file is executed by most X session managers), e.g.

    #!/bin/bash
    /path/to/wacom-output-mapping-watcher -o LVDS1 -w &
        # In this example, LVDS1 is the name of X Output corresponding to laptop monitor
        # Use `xrandr -q' to determine which X Output name to use

## Compilation

`wacom-output-mapping-watcher` is written in [Rust](http://www.rust-lang.org/). To compile, invoke:

    cargo build

(tested with Rust 1.1)
