.PHONY: all install

all:
	@./x.sh

install:
	cp dist/guppyctl /usr/local/bin/guppyctl
