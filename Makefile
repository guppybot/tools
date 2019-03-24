.PHONY: all install

all:
	@./x.sh

install:
	cp dist/guppybot /usr/local/bin/guppybot
	cp dist/guppyctl /usr/local/bin/guppyctl
