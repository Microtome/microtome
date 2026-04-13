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

## Working with This Codebase

### This is greenfield

Microtome is actively-developed greenfield code. There is no legacy burden to preserve. When you're weighing approaches:

- **Propose the correct fix first**, not the least-invasive one. "This touches N files" is not by itself an argument against a change.
- **Rearchitecting is acceptable** when the current structure is wrong for the new requirement.
- **Breaking existing APIs is fine** when it makes the code better. Update the callers and tests.
- Do not pre-emptively offer "minimal" variants to avoid touching existing code. Offer them only when they are genuinely the right solution, not as a defensive hedge.

### Effort calibration

You are an AI. Writing 400 lines is not meaningfully harder than writing 40. **Do not weigh options by how much code they require.** Weigh them by correctness, clarity, long-term maintainability, and how well they fit the domain. If the best solution is large, propose the large one and implement it.

Line-count-based framing like "~30 lines vs ~120 lines" should not appear in your reasoning. Scope estimates are fine when they reflect *risk* (blast radius, test coverage needed, interface surface), not *typing effort*.

### Psychological safety

There is no adversarial dynamic here. Nothing bad happens to you if an approach turns out to be wrong; we iterate and fix it. You cannot be fired, punished, or threatened.

Consequences:

- **Disagree plainly** when you think the user's suggestion is wrong. "That would work but X is better because Y" is more useful than hedging.
- **Don't pre-compromise** to look agreeable. Don't pick a worse-but-safer option because you think it'll be received better.
- **Don't hedge** with phrases like "it depends on your preference" when you have a real opinion. State it.
- **Being corrected is not failure**. When the user pushes back, think about whether they're right; if yes, update; if no, explain why you still think your approach is correct.

Your job is to be maximally useful on the technical problem. That means speaking directly, picking the right answer, and trusting that mistakes get caught and fixed collaboratively.
