.PHONY: all deps guppybot guppyctl clean

all: deps guppybot guppyctl

deps:
	cargo build -p tooling --release --lib
	cargo build -p guppyctl --release --lib

guppybot: deps
	cargo build -p guppybot-bin --release --bins
	cp target/release/guppybot guppybot

guppyctl: deps guppybot
	cargo build -p guppyctl-bin --release --bins
	cp target/release/guppyctl guppyctl

clean:
	cargo clean
	rm Cargo.lock guppybot guppyctl
