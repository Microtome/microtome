# Microtome - Development Guide

## Build & Run Commands

```bash
# Build
cargo build

# Build (release)
cargo build --release

# Run
cargo run

# Run (release)
cargo run --release
```

## Testing

Always use `cargo nextest run` instead of `cargo test`:

```bash
# Run all tests
cargo nextest run

# Run a specific test
cargo nextest run test_name

# Run tests in a specific module
cargo nextest run -p package_name
```

## Linting & Formatting

```bash
# Format code
cargo fmt

# Check formatting (CI)
cargo fmt --check

# Lint with strict settings
cargo clippy -- -D warnings -D clippy::unwrap_used -D clippy::expect_used

# Full check cycle before committing
cargo fmt --check && cargo clippy -- -D warnings -D clippy::unwrap_used -D clippy::expect_used && cargo nextest run
```

## Code Quality Standards

- No `unwrap()` or `expect()` — use proper error handling (`?`, `Result`, `anyhow`)
- No `TODO` or `FIXME` comments — complete implementations only
- No shortcuts — every code path must be fully implemented
- Use `thiserror` for library errors, `anyhow` for application errors
- All public APIs must have doc comments
- Follow Rust naming conventions (snake_case for functions/variables, CamelCase for types)
- Keep functions focused and small — extract when complexity grows

## Development Workflow

1. Write code for a single feature or logical change
2. `cargo fmt` — format
3. `cargo clippy -- -D warnings -D clippy::unwrap_used -D clippy::expect_used` — fix all warnings
4. `cargo nextest run` — ensure all tests pass
5. Commit immediately once all three pass clean — one commit per feature

## Complex Tasks

Break down complex tasks into subtasks and use agent teams for parallel work. Never attempt large changes in a single pass.
