.PHONY: build test install uninstall

PREFIX ?= $(HOME)/.local
BINDIR ?= $(PREFIX)/bin
DATADIR ?= $(PREFIX)/share

build:
	cargo build --release

test:
	cargo test

install: build
	install -Dm755 target/release/flint "$(BINDIR)/flint"
	install -Dm644 share/flint.desktop "$(DATADIR)/applications/flint.desktop"
	install -Dm644 share/icons/flint-32.png "$(DATADIR)/icons/hicolor/32x32/apps/flint.png"
	install -Dm644 share/icons/flint-48.png "$(DATADIR)/icons/hicolor/48x48/apps/flint.png"
	install -Dm644 share/icons/flint-64.png "$(DATADIR)/icons/hicolor/64x64/apps/flint.png"
	install -Dm644 share/icons/flint-128.png "$(DATADIR)/icons/hicolor/128x128/apps/flint.png"
	install -Dm644 share/icons/flint-256.png "$(DATADIR)/icons/hicolor/256x256/apps/flint.png"
	install -Dm644 share/icons/flint-512.png "$(DATADIR)/icons/hicolor/512x512/apps/flint.png"
	install -Dm644 share/flint.svg "$(DATADIR)/icons/hicolor/scalable/apps/flint.svg"
	-update-desktop-database "$(DATADIR)/applications" 2>/dev/null
	-gtk-update-icon-cache -f -t "$(DATADIR)/icons/hicolor" 2>/dev/null
	@echo "Installed $(BINDIR)/flint"

uninstall:
	rm -f "$(BINDIR)/flint"
	rm -f "$(DATADIR)/applications/flint.desktop"
	rm -f "$(DATADIR)/icons/hicolor/"*/apps/flint.png
	rm -f "$(DATADIR)/icons/hicolor/scalable/apps/flint.svg"
