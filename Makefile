.DEFAULT_GOAL := help

CARGO ?= cargo
DX ?= dx
CLI_ARGS ?=
API_BIND ?= 0.0.0.0:8080
MOBILE_DEV_PORT ?= 8081
DESKTOP_DEV_PORT ?= 8082

define HELP_TEXT
Dirt workspace commands

  fmt                      Format all Rust code
  lint                     Run clippy for all targets
  check-all                Check all crates
  test-all                 Run full workspace tests
  ci                       Run local CI checks
  clean                    Clean build artifacts

  check-core               Check dirt-core
  test-core                Test dirt-core
  build-core               Build dirt-core (release)

  check-cli                Check dirt-cli
  test-cli                 Test dirt-cli
  run-cli                  Run dirt-cli (CLI_ARGS='...')
  build-cli                Build dirt-cli (release)

  check-api                Check dirt-api
  test-api                 Test dirt-api
  run-api                  Run dirt-api (API_BIND=0.0.0.0:8080)
  build-api                Build dirt-api (release)

  check-desktop            Check dirt-desktop
  test-desktop             Test dirt-desktop
  run-desktop              Run desktop app via Dioxus (DESKTOP_DEV_PORT=8082)
  build-desktop            Build desktop app (release)

  check-mobile             Check dirt-mobile
  test-mobile              Test dirt-mobile
  run-mobile-android       Run mobile app on Android (MOBILE_DEV_PORT=8081)
  run-mobile-ios           Run mobile app on iOS (MOBILE_DEV_PORT=8081)
  run-mobile-web           Run mobile package on web (MOBILE_DEV_PORT=8081)
  build-mobile-android     Build Android release

  check-platforms          Check API + CLI + desktop + mobile
  test-platforms           Test core + API + CLI + desktop + mobile
  run-platforms            Print common local run commands
endef

define RUN_PLATFORMS_TEXT
Desktop: make run-desktop
API:     make run-api
CLI:     make run-cli CLI_ARGS="--help"
Android: make run-mobile-android
iOS:     make run-mobile-ios
endef

.PHONY: \
	help \
	fmt lint \
	check check-all test test-all ci clean \
	check-core test-core build-core \
	check-cli test-cli run-cli build-cli \
	check-api test-api run-api build-api \
	check-desktop test-desktop run-desktop build-desktop \
	check-mobile test-mobile run-mobile-android run-mobile-ios run-mobile-web build-mobile-android \
	check-platforms test-platforms run-platforms

help: ## Show available targets
	$(info $(HELP_TEXT))
	@:

fmt: ## Format all Rust code
	$(CARGO) fmt --all

lint: ## Run clippy for all targets
	$(CARGO) clippy --workspace --all-targets

check: check-all ## Alias for check-all

check-all: ## Check all crates
	$(CARGO) check --workspace --all-targets

test: test-all ## Alias for test-all

test-all: ## Run full workspace tests
	$(CARGO) test --workspace

ci: fmt lint test-all ## Run local CI checks

clean: ## Clean build artifacts
	$(CARGO) clean

check-core: ## Check dirt-core
	$(CARGO) check -p dirt-core

test-core: ## Test dirt-core
	$(CARGO) test -p dirt-core

build-core: ## Build dirt-core (release)
	$(CARGO) build -p dirt-core --release

check-cli: ## Check dirt-cli
	$(CARGO) check -p dirt-cli

test-cli: ## Test dirt-cli
	$(CARGO) test -p dirt-cli

run-cli: ## Run dirt-cli (pass args with: make run-cli CLI_ARGS="notes list")
	$(CARGO) run -p dirt-cli -- $(CLI_ARGS)

build-cli: ## Build dirt-cli (release)
	$(CARGO) build -p dirt-cli --release

check-api: ## Check dirt-api
	$(CARGO) check -p dirt-api

test-api: ## Test dirt-api
	$(CARGO) test -p dirt-api

run-api: export DIRT_API_BIND_ADDR := $(API_BIND)
run-api: ## Run dirt-api (expects .env.server or equivalent env vars)
	$(CARGO) run -p dirt-api

build-api: ## Build dirt-api (release)
	$(CARGO) build -p dirt-api --release

check-desktop: ## Check dirt-desktop
	$(CARGO) check -p dirt-desktop

test-desktop: ## Test dirt-desktop
	$(CARGO) test -p dirt-desktop

run-desktop: ## Run desktop app (Dioxus dev server)
	$(DX) serve --platform desktop --package dirt-desktop --port $(DESKTOP_DEV_PORT)

build-desktop: ## Build desktop app (release)
	$(CARGO) build -p dirt-desktop --release

check-mobile: ## Check dirt-mobile
	$(CARGO) check -p dirt-mobile

test-mobile: ## Test dirt-mobile
	$(CARGO) test -p dirt-mobile

run-mobile-android: ## Run mobile app on Android (Dioxus)
	$(DX) serve --platform android --package dirt-mobile --port $(MOBILE_DEV_PORT)

run-mobile-ios: ## Run mobile app on iOS (Dioxus)
	$(DX) serve --platform ios --package dirt-mobile --port $(MOBILE_DEV_PORT)

run-mobile-web: ## Run mobile package on web target (quick UI validation)
	$(DX) serve --platform web --package dirt-mobile --port $(MOBILE_DEV_PORT)

build-mobile-android: ## Build Android APK/AAB (release)
	$(DX) build --platform android --package dirt-mobile --release

check-platforms: check-api check-cli check-desktop check-mobile ## Check all app platforms

test-platforms: test-core test-api test-cli test-desktop test-mobile ## Test all platform crates

run-platforms: ## Show common local run entrypoints
	$(info $(RUN_PLATFORMS_TEXT))
	@:
