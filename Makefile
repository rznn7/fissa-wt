.PHONY: build run test lint install clean help

build:
	cargo build

run:
	cargo run

test:
	cargo test

lint:
	cargo fmt --check
	cargo clippy -- -D warnings

install:
	cargo install --path .

clean:
	cargo clean

help:
	@echo "Available targets:"
	@echo "  make build    - Build the project"
	@echo "  make run      - Run the project"
	@echo "  make test     - Run the test suite"
	@echo "  make lint     - Check formatting and lints"
	@echo "  make install  - Install the binary with cargo"
	@echo "  make clean    - Remove build artifacts"
