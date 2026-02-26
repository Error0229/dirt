.PHONY: fmt lint check test build run clean ci api mobile-android dev-android help

EMULATOR_API_BASE_URL ?= http://10.0.2.2:8080
EMULATOR_BOOTSTRAP_URL ?= $(EMULATOR_API_BASE_URL)/v1/bootstrap
EMULATOR_SYNC_TOKEN_ENDPOINT ?= $(EMULATOR_API_BASE_URL)/v1/sync/token

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

# Run backend API (loads .env if present)
api:
	@sh -c 'set -a; [ -f .env ] && . ./.env; set +a; cargo run -p dirt-api'

# Run Android app with emulator-safe API URLs (requires SUPABASE_URL + SUPABASE_ANON_KEY in .env)
mobile-android:
	@sh -c 'set -a; [ -f .env ] && . ./.env; set +a; \
		test -n "$$SUPABASE_URL" || { echo "SUPABASE_URL is required (.env)"; exit 1; }; \
		test -n "$$SUPABASE_ANON_KEY" || { echo "SUPABASE_ANON_KEY is required (.env)"; exit 1; }; \
		DIRT_API_BASE_URL="$(EMULATOR_API_BASE_URL)" \
		DIRT_BOOTSTRAP_URL="$(EMULATOR_BOOTSTRAP_URL)" \
		TURSO_SYNC_TOKEN_ENDPOINT="$(EMULATOR_SYNC_TOKEN_ENDPOINT)" \
		SUPABASE_URL="$$SUPABASE_URL" \
		SUPABASE_ANON_KEY="$$SUPABASE_ANON_KEY" \
		dx serve --platform android --package dirt-mobile'

# Start API + Android app together (Ctrl+C stops both)
dev-android:
	@sh -c 'set -a; [ -f .env ] && . ./.env; set +a; \
		test -n "$$SUPABASE_URL" || { echo "SUPABASE_URL is required (.env)"; exit 1; }; \
		test -n "$$SUPABASE_ANON_KEY" || { echo "SUPABASE_ANON_KEY is required (.env)"; exit 1; }; \
		cargo run -p dirt-api & api_pid=$$!; \
		trap "kill $$api_pid 2>/dev/null || true" EXIT INT TERM; \
		DIRT_API_BASE_URL="$(EMULATOR_API_BASE_URL)" \
		DIRT_BOOTSTRAP_URL="$(EMULATOR_BOOTSTRAP_URL)" \
		TURSO_SYNC_TOKEN_ENDPOINT="$(EMULATOR_SYNC_TOKEN_ENDPOINT)" \
		SUPABASE_URL="$$SUPABASE_URL" \
		SUPABASE_ANON_KEY="$$SUPABASE_ANON_KEY" \
		dx serve --platform android --package dirt-mobile'

# Clean build artifacts
clean:
	cargo clean

# All checks (fmt + lint + test)
ci: fmt lint test

help:
	@echo "Targets:"
	@echo "  make api              - run dirt-api with .env"
	@echo "  make mobile-android   - run Android app with emulator env"
	@echo "  make dev-android      - run API + Android app together"
	@echo "  make run              - run desktop app"
	@echo "  make ci               - fmt + lint + test"
