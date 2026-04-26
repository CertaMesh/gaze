/// Install a panic hook that prints a sanitized error line and exits 3.
/// Without this, a panic in `ort`, `regex`, or any other dep would leak a raw
/// backtrace to stderr whenever `RUST_BACKTRACE` is set — violating the
/// stderr discipline captured in `ROADMAP.md`.
pub(crate) fn install_panic_hook() {
    std::panic::set_hook(Box::new(|_info| {
        eprintln!(r#"{{"error":"Pipeline","exit":3}}"#);
        // Force exit 3 so the host wrapper sees the documented code instead
        // of Rust's default 101. The hook runs BEFORE the runtime unwinds,
        // so `process::exit` here is the only way to guarantee both the
        // sanitized stderr line AND the contracted exit code.
        std::process::exit(3);
    }));
}
