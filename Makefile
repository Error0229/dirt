.PHONY: fmt lint check test build run clean

# Format code
fmt:
	cargo fmt

# Run clippy
lint:
	cargo clippy --all-targets

# Check without building
check:
	cargo check --all-targets

# Run tests
test:
	cargo test

# Build release
build:
	cargo build --release

# Run desktop app
run:
	dx serve --platform desktop --package dirt-desktop

# Clean build artifacts
clean:
	cargo clean

# All checks (fmt + lint + test)
ci: fmt lint test
