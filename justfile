# Lynxby Desktop. Run `just` for the list.

set shell := ["bash", "-c"]

default:
    @just --list

# Format, lint and test — the same checks CI runs.
check: fmt clippy test

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

clippy:
    cargo clippy --workspace --all-targets -- -D warnings

test:
    cargo test --workspace

# Unit and integration tests only, with output on failure.
test-watch:
    cargo watch -x 'test --workspace'

# Talk to ddev.
run:
    cargo run

# Talk to production.
run-prod:
    cargo run -- --env prod

# The assistant list with sample data, no deployment needed.
demo:
    LYNXBY_DEMO=1 cargo run

demo-chat:
    LYNXBY_DEMO=chat cargo run

# Read-only smoke tests against a running deployment.
test-ddev:
    LYNXBY_TEST_BASE_URL=https://projectsovereign.ddev.site cargo test -p lynxby-client --test ddev -- --ignored --nocapture

# Release build with the app's own symbols stripped.
build-release:
    cargo build --release
