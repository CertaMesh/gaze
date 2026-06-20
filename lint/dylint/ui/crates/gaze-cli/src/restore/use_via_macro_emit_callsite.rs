#[macro_use]
#[path = "../macros/macro_emit.inc"]
mod macro_emit;

emit_forbidden_audit_use!();

fn main() {
    let _ = SqliteLogger::new();
}
