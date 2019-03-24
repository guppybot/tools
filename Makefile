PREFIX ?= /usr/local

.PHONY: all install

all:
	@./x.sh

install:
	cp dist/guppybot $(PREFIX)/bin/guppybot
	cp dist/guppyctl $(PREFIX)/bin/guppyctl
