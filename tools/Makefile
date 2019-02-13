.PHONY: all deps guppyctl clean

all: deps guppyctl

deps:
	cargo build -p tooling --release --lib
	cargo build -p guppyctl --release --lib

guppyctl: deps
	cargo build -p guppyctl-bin --release --bins
	cp target/release/guppyctl guppyctl

clean:
	cargo clean
	rm Cargo.lock
