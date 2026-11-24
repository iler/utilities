BIN := dep-up
PREFIX ?= $(HOME)/.local

.DEFAULT_GOAL := help
.PHONY: help build release run test live check fmt fmt-check lint install uninstall clean

help: ## Show this help
	@grep -E '^[a-z-]+:.*##' $(MAKEFILE_LIST) | awk -F':.*## ' '{printf "  %-10s %s\n", $$1, $$2}'

build: ## Debug build
	cargo build

release: ## Optimized build (target/release/dep-up)
	cargo build --release

run: ## Run the TUI (pass args with ARGS="--only brew")
	cargo run -- $(ARGS)

test: ## Run the unit tests
	cargo test

live: ## Run the real checks of this machine (they change nothing)
	cargo test live_checks -- --ignored --nocapture

check: fmt-check lint test ## Format check, clippy and tests

fmt: ## Format the code
	cargo fmt

fmt-check:
	cargo fmt --check

lint: ## Run clippy with warnings as errors
	cargo clippy --all-targets -- -D warnings

install: release ## Install to $(PREFIX)/bin
	install -d $(PREFIX)/bin
	install -m 755 target/release/$(BIN) $(PREFIX)/bin/$(BIN)

uninstall: ## Remove from $(PREFIX)/bin
	rm -f $(PREFIX)/bin/$(BIN)

clean: ## Remove the build output
	cargo clean
