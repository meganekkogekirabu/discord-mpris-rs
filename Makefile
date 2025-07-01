.PHONY: all build install uninstall

build:
	cargo build --release

install:
	mkdir -p /usr/share/discord-mpris-rs
	cp target/release/discord-mpris-rs /usr/local/bin/discord-mpris-rs
	chmod +x /usr/local/bin/discord-mpris-rs
	cp .env /usr/share/discord-mpris-rs/.env
	curl -o discord-mpris-rs.service https://aur.archlinux.org/cgit/aur.git/plain/discord-mpris-rs.service?h=discord-mpris-rs 
	cp discord-mpris-rs.service /usr/lib/systemd/user/
	systemctl --user enable discord-mpris-rs.service
	systemctl --user start discord-mpris-rs.service
	cargo clean

uninstall:
	rm -f /usr/local/bin/discord-mpris-rs
	rm -rf /usr/share/discord-mpris-rs
	systemctl disable --user discord-mpris-rs
	systemctl --user daemon-reload
	rm -f /usr/lib/systemd/user/discord-mpris-rs.service