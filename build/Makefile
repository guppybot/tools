CARGO_BUILD := cargo build --manifest-path ../tools/Cargo.toml --target-dir ./target --release

.PHONY: all deps guppybot guppyctl clean

all: deps guppybot guppyctl

deps:
	$(CARGO_BUILD) -p tooling --lib
	$(CARGO_BUILD) -p guppyctl --lib

guppybot: deps
	$(CARGO_BUILD) -p guppybot-bin --bins
	cp target/release/guppybot guppybot

guppyctl: deps guppybot
	$(CARGO_BUILD) -p guppyctl-bin --bins
	cp target/release/guppyctl guppyctl

clean:
	cargo clean
	rm -f Cargo.lock guppybot guppyctl
