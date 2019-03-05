.PHONY: all install

all: dist/guppyctl

install: dist/guppyctl
	cp dist/guppyctl /usr/local/bin/guppyctl

dist/guppyctl:
	make -C build
	mkdir -p dist
	cp build/guppyctl dist/guppyctl
