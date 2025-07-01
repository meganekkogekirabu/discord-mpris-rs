# discord-mpris-rs

Stupid name.

Discord Rich Presence integration for MPRIS2-compatible media players, such as VLC or Lollypop, written in Rust.

This is intended to be an alternative to other similar programs in that it's more meaningfully customisable, letting you change the application used from the built-in one to your own, ignore certain media players, changed what's actually displayed in the Rich Presence widget, etc.

The icons for the media players have to be uploaded to the Discord application, so open an issue if you want a new one uploaded.


## Installation

```sh
git clone https://github.com/meganekkogekirabu/discord-mpris-rs
cd discord-mpris-rs
make
```

It can be configured through ~/.config/discord-mpris-rs/.env. Configuration changes require you to restart the service.

To uninstall, run `make uninstall`.