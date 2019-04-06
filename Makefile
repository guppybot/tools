PREFIX ?= /usr/local
USER_PREFIX ?= $(HOME)/.guppybot

.PHONY: all install install-user

all:
	@./x.sh

install:
	cp dist/guppybot $(PREFIX)/bin/guppybot
	cp dist/guppyctl $(PREFIX)/bin/guppyctl

install-user:
	mkdir -p $(USER_PREFIX)/bin
	cp dist/guppybot $(USER_PREFIX)/bin/guppybot
	cp dist/guppyctl $(USER_PREFIX)/bin/guppyctl
