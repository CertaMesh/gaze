---
paths: "**/*.rs"
---

# Rust Rules

Core principle: **let the type system prevent bugs at compile time** rather than catching them at runtime.

## Project Structure

Use a virtual workspace manifest (no `[package]` in root `Cargo.toml`) with a flat `crates/` layout:

```
project/
├── Cargo.toml          # Virtual workspace manifest
├── Cargo.lock          # Committed for binaries
├── deny.toml
├── clippy.toml
├── rustfmt.toml
├── crates/
│   ├── core/           # Business logic (lib)
│   ├── cli/            # CLI binary (bin)
│   └── daemon/         # Daemon binary (bin)
└── xtask/              # Build automation
```

### Workspace Configuration

Centralize ALL dependency versions and lints in root `Cargo.toml`:

```toml
[workspace]
resolver = "3"
members = ["crates/*"]

[workspace.package]
edition = "2024"
rust-version = "1.85"

[workspace.dependencies]
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
thiserror = "2"
anyhow = "1"
# Internal crates
my-core = { path = "crates/core" }

[workspace.lints.rust]
unsafe_code = "forbid"
unreachable_pub = "warn"
unused_must_use = "deny"

[workspace.lints.clippy]
pedantic = { level = "warn", priority = -1 }
module_name_repetitions = "allow"
must_use_candidate = "allow"
missing_errors_doc = "allow"
missing_panics_doc = "allow"
dbg_macro = "deny"
print_stdout = "deny"
print_stderr = "deny"
unwrap_used = "deny"
todo = "deny"
unimplemented = "deny"
undocumented_unsafe_blocks = "deny"
clone_on_ref_ptr = "warn"
string_to_string = "warn"
wildcard_dependencies = "warn"
```

Member crates inherit:

```toml
[package]
name = "my-core"
version.workspace = true
edition.workspace = true

[dependencies]
serde = { workspace = true }

[lints]
workspace = true
```

### Crate Splitting Rules

- Split when a module has clear independent responsibility or to improve compile parallelism
- Keep binary crates thin — just `main()` + CLI arg parsing; all logic in library crates
- Use `pub(crate)` for internal APIs, enable `unreachable_pub` lint
- Use `version = "0.0.0"` for internal crates you don't publish

### Build Profiles

```toml
[profile.release]
opt-level = 3
lto = "thin"
codegen-units = 1
strip = "symbols"
panic = "abort"

[profile.dev.package."*"]
opt-level = 3          # Optimize deps in dev mode
```

## Error Handling

**Boundary rule**: `thiserror` for library crates (typed, matchable), `anyhow` for binaries (opaque, ergonomic).

```rust
// Library: typed errors
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to read configuration file")]
    ReadFailed(#[from] std::io::Error),
    #[error("invalid value for key `{key}`")]
    InvalidValue { key: String, #[source] source: ParseError },
}

// Binary: anyhow with context
use anyhow::{Context, Result};

fn load_config(path: &Path) -> Result<Config> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    toml::from_str(&content).context("failed to parse configuration")
}
```

### Error Message Conventions

- Lowercase, no trailing punctuation
- Describe only the current layer — don't embed the source in Display
- Log errors only at the resolution point (avoid duplicate logging up the chain)

### .unwrap() and .expect() Rules

- **Never** in production code (enforced by `clippy::unwrap_used`)
- `.expect()` only for proven invariants: `Regex::new(r"^\d+$").expect("static regex")`
- Both are fine in tests

## Async Rust (Tokio)

### Never Block the Runtime

No `std::fs`, `std::thread::sleep`, or CPU-heavy work on Tokio workers. Rule of thumb: no more than 10-100 microseconds between `.await` points.

| Workload | `spawn_blocking` | `rayon` | Dedicated thread |
|----------|:-:|:-:|:-:|
| CPU-bound | Suboptimal | Yes | Yes |
| Sync I/O | Yes | No | Yes |
| Runs forever | No | No | Yes |

```rust
// File I/O: use spawn_blocking
let data = tokio::task::spawn_blocking(move || {
    std::fs::read_to_string(path)
}).await??;

// CPU parallelism: bridge rayon with oneshot
let (tx, rx) = tokio::sync::oneshot::channel();
rayon::spawn(move || { let _ = tx.send(expensive_compute()); });
let result = rx.await?;
```

### Structured Concurrency

Use `JoinSet` (owns tasks, gives return values, aborts on drop) or `TaskTracker` (tracks tasks for shutdown, does NOT abort on drop).

```rust
// Graceful shutdown pattern
let token = CancellationToken::new();
let tracker = TaskTracker::new();

while let Some(conn) = token.run_until_cancelled(listener.accept()).await {
    let child_token = token.child_token();
    tracker.spawn(async move { handle(conn?, child_token).await });
}
tracker.close();
tracker.wait().await;
```

### Channel Selection

| Channel | Pattern | Use when |
|---------|---------|----------|
| `mpsc` | Many-to-One, buffered | Work queues, actor mailboxes |
| `oneshot` | One-to-One, single msg | Request/response, task results |
| `broadcast` | Many-to-Many, all see all | Event bus, pub/sub |
| `watch` | Many-to-Many, latest only | Config updates, state snapshots |

Always use **bounded** `mpsc` channels. Never use `std::sync::mpsc` in async code.

### Cancellation Safety

Cancel-safe in `select!`: `recv()`, `join_next()`, `cancelled()`.
**Not** cancel-safe: `read_line()`, `read_exact()`, `send()`. Use `reserve().await` + `permit.send()` instead.

### Mutex Rules

- `std::sync::Mutex` is fine and faster in async code for short critical sections
- Only use `tokio::sync::Mutex` when holding across `.await` points

### async fn in Traits

- Native `async fn` in traits works since Rust 1.75 (all editions) for static dispatch
- Use `#[trait_variant::make(SendTrait: Send)]` for Send bounds
- Keep `async-trait` only for `dyn Trait` (dynamic dispatch)

## CLI Design (clap)

```rust
#[derive(Debug, Parser)]
#[command(name = "myapp", version, about, propagate_version = true)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalOpts,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Args)]
pub struct GlobalOpts {
    #[arg(long, value_enum, global = true, default_value_t = OutputFormat::Human)]
    pub format: OutputFormat,
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,
}
```

- Top-level `Parser` struct, never an enum (blocks adding global options later)
- Use `value_parser` for validation — constraints appear in help text
- Data to stdout, errors to stderr
- Don't use `fn main() -> Result<()>` — use explicit error handling + `std::process::exit()`

## Serialization (serde)

```rust
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ServerConfig {
    pub listen_addr: String,
    #[serde(default = "default_port")]
    pub listen_port: u16,
    #[serde(default)]
    pub tls_cert: Option<PathBuf>,
}
```

- Use `rename_all = "kebab-case"` + `deny_unknown_fields` for TOML configs
- Use `#[serde(default)]` on new optional fields for forward compatibility
- Don't use `deny_unknown_fields` with `#[serde(flatten)]` — they conflict

## Observability (tracing)

```rust
let filter = EnvFilter::try_from_default_env()
    .unwrap_or_else(|_| EnvFilter::new("info,my_crate=debug"));
tracing_subscriber::registry()
    .with(filter)
    .with(fmt::layer().with_target(true))
    .init();
```

### Log Levels

| Level | Use for |
|-------|---------|
| `error!` | Unrecoverable failures requiring action |
| `warn!` | Unexpected but recoverable situations |
| `info!` | Key lifecycle events (started, loaded, stopped) |
| `debug!` | Developer diagnostics |
| `trace!` | Hot-path details |

### `#[instrument]` Best Practices

- Always `skip(self)` on methods
- `skip(password, api_key)` for sensitive data
- Use `err` to auto-log errors
- Use `level = "trace"` for hot internal functions
- Use `.instrument(span)` on spawned tasks, never `span.enter()` across `.await`

### Structured Logging

```rust
// DO: structured fields
tracing::error!(error = %err, user_id = %uid, "payment processing failed");

// DON'T: format strings
tracing::error!("payment failed for user {} with error: {}", uid, err);
```

## Type System and API Design

### Newtype Pattern

```rust
pub struct UserId(u64);
pub struct OrderId(u64);
// These are distinct types — can't accidentally mix them
```

### Typestate Pattern

```rust
struct Connection<State> { _state: PhantomData<State> }
struct Disconnected;
struct Connected;

impl Connection<Disconnected> {
    fn connect(self) -> Connection<Connected> { /* ... */ }
}
impl Connection<Connected> {
    fn send(&self, data: &[u8]) { /* ... */ }
}
// No send() on Disconnected — compile error
```

### Conversion Traits

- Always implement `From<T>`, never `Into<T>` (blanket impl provides it)
- Use `TryFrom<T>` for fallible conversions
- Method naming: `as_` (cheap borrow), `to_` (expensive/borrow-to-owned), `into_` (ownership transfer)
- Getters: `fn field(&self) -> &T` (no `get_` prefix)

### Generics vs Trait Objects

- Generics (`impl Trait` / `<T>`) for hot paths (static dispatch, inlining)
- `dyn Trait` for heterogeneous collections or plugin architectures

## Parameter Types

Accept borrowed types in function signatures:

| Accept | Not |
|--------|-----|
| `&str` | `String` |
| `&[T]` | `Vec<T>` |
| `&Path` | `PathBuf` |
| `impl AsRef<str>` | `String` |
| `Cow<'_, str>` | When sometimes need to allocate |

## Process Supervision (Unix)

### Signal Handling

Use `tokio::signal::unix::signal` for async signals. Handle `SIGTERM`/`SIGINT` for shutdown, `SIGHUP` for reload.

### PID Files

Use file locking (`flock`) with RAII `Drop` cleanup. Never check-then-create (TOCTOU race).

### Process Groups

Use `command-group` with `group_spawn()`. Kill the group, not just the parent PID — prevents orphans.

### Zombie Prevention

Reap children with `waitpid(Pid::from_raw(-1), Some(WaitPidFlag::WNOHANG))` in a loop.

## Safety and Security

- `unsafe_code = "forbid"` at workspace level
- Every `unsafe` block needs `// SAFETY:` comment
- Use `zeroize::Zeroizing<T>` for secrets, `secrecy::SecretString` for secret strings
- Set file permissions at creation: `OpenOptions::mode(0o600)`
- Never use `set_readonly(false)` on Unix (makes files world-writable)
- Use `tempfile::tempfile()` for temporary files (unnamed, immediately unlinked)
- No shell command construction from user input — pass args directly
- CI: `cargo audit` + `cargo deny` + `cargo vet`

## Testing

### Structure

- Unit tests: `#[cfg(test)] mod tests` at bottom of source files
- Integration tests: `tests/` with `common/mod.rs` for shared helpers
- Async tests: `#[tokio::test]` with `start_paused = true` for time-dependent tests

### Mocking

Design with trait-based dependency injection:

```rust
#[cfg_attr(test, mockall::automock)]
pub trait UserRepository: Send + Sync {
    async fn find_by_id(&self, id: u64) -> Result<User, DbError>;
}
```

### CLI Testing

Use `assert_cmd` + `predicates` for testing exit codes, stdout/stderr content.

## Naming Conventions (RFC 430)

| Item | Convention | Example |
|------|-----------|---------|
| Types, traits, enum variants | `UpperCamelCase` | `HttpRequest`, `NotFound` |
| Functions, methods, modules | `snake_case` | `read_file`, `is_empty` |
| Constants, statics | `SCREAMING_SNAKE_CASE` | `MAX_CONNECTIONS` |
| Type parameters | Single uppercase | `T`, `E`, `K` |
| Lifetimes | Short lowercase | `'a`, `'de` |

Acronyms as one word: `Uuid` not `UUID`, `Stdin` not `StdIn`.

## Module Organization

- Use `filename.rs` style (not `mod.rs`) for new projects
- Keep `lib.rs`/`main.rs` to module declarations and re-exports
- Use `pub use` re-exports to flatten API surface

## Documentation

- `///` for items, `//!` for crate/module-level docs
- First line: short summary sentence
- Required sections: `# Examples` (public items), `# Errors` (Result fns), `# Panics`, `# Safety` (unsafe fns)

## Tooling Configuration

### rustfmt.toml

```toml
edition = "2024"
style_edition = "2024"
max_width = 100
hard_tabs = false
tab_spaces = 4
newline_style = "Unix"
```

### clippy.toml

```toml
msrv = "1.85.0"
avoid-breaking-exported-api = true
cognitive-complexity-threshold = 30
too-many-arguments-threshold = 8
allow-unwrap-in-tests = true
allow-expect-in-tests = true
allow-dbg-in-tests = true
allow-print-in-tests = true
large-error-threshold = 256
```

## Edition 2024 Key Changes

- **RPIT captures all lifetimes** — use `+ use<>` to opt out
- **`unsafe_op_in_unsafe_fn`** is warn-by-default
- **`unsafe extern`** blocks required
- **Unsafe attributes**: `#[unsafe(no_mangle)]`
- **`static mut` references** are errors — use `Mutex`/`AtomicXxx`
- **`gen` keyword reserved** — use `r#gen` for old code
- **Prelude**: `Future` and `IntoFuture` included
- **`std::env::set_var`** is now `unsafe fn`
- **`style_edition = "2024"`** required in `rustfmt.toml`

## Common Mistakes

| Mistake | Fix |
|---------|-----|
| Blocking async runtime with `std::fs` | Use `tokio::fs` or `spawn_blocking` |
| `.unwrap()` in production code | Use `?` with `.context()` |
| Cancel-unsafe futures in `select!` | Use `reserve()` + `permit.send()` |
| `tokio::spawn` without tracking | Use `TaskTracker` or `JoinSet` |
| `std::sync::Mutex` across `.await` | Use `tokio::sync::Mutex` |
| `type Alias = u64` for domain types | Use `struct NewType(u64)` |
| `String` params when `&str` works | Accept borrowed types |
| `fn main() -> Result<()>` in CLI | Explicit error handling + `exit()` |
| `format!()` in hot loops | Use `write!` with reusable buffer |
| `deny_unknown_fields` + `flatten` | They conflict — remove one |
| `anyhow::Error` in library public API | Use `thiserror` typed errors |
| Unbounded `mpsc` channels | Use bounded with explicit capacity |
| `async-trait` on Rust >= 1.75 | Use native `async fn` in traits |
| `set_readonly(false)` on Unix | World-writable — use explicit `mode()` |
| Missing `style_edition` in rustfmt.toml | Editor format-on-save uses wrong style |

## CI Pipeline

Essential checks in order (fastest first):

1. `cargo fmt --all -- --check`
2. `cargo clippy --workspace --all-targets --all-features` (with `RUSTFLAGS="-Dwarnings"`)
3. `cargo nextest run --workspace --all-features` + `cargo test --doc --workspace`
4. `cargo deny check`
5. `cargo machete` (unused dependencies)
