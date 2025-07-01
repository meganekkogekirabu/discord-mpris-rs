# discord-mpris-rs

Stupid name.

Discord Rich Presence integration for MPRIS2-compatible media players, such as VLC or Lollypop, written in Rust.

This is intended to be an alternative to other similar programs in that it's more meaningfully customisable, letting you change the application used from the built-in one to your own, ignore certain media players, change what's actually displayed in the Rich Presence widget, etc.


## Installation

Available through the [Arch User Repository](https://aur.archlinux.org/packages/discord-mpris-rs).

If you are not on Arch Linux, you can also clone and manually install, which will require you to 1) build with `cargo build --release`, 2) download `discord-mpris-rs.service` from the AUR package, and 3) copy everything to the appropriate directories.

It can be configured through /usr/share/discord-mpris-rs/.env. Configuration changes require you to restart the service.