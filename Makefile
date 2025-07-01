.PHONY: all build install uninstall
.DEFAULT_GOAL := all

all: build install

build:
	cargo build --release

install: build
	mkdir -p ~/.local/bin
	mkdir -p ~/.config/discord-mpris-rs
	cp target/release/discord-mpris-rs ~/.local/bin/discord-mpris-rs
	chmod +x ~/.local/bin/discord-mpris-rs
	cp .env ~/.config/discord-mpris-rs/.env
	cp systemd/discord-mpris-rs.service ~/.config/systemd/user/discord-mpris-rs.service
	systemctl --user enable discord-mpris-rs.service
	systemctl --user start discord-mpris-rs.service
	cargo clean

uninstall:
	rm -f ~/.local/bin/discord-mpris-rs
	rm -rf ~/.config/discord-mpris-rs
	systemctl disable --user discord-mpris-rs
	systemctl --user daemon-reload
	rm -f ~/.config/systemd/user/discord-mpris-rs.service