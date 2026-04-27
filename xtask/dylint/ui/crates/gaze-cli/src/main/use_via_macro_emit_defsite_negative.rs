#[macro_use]
#[path = "../restore/macro_defsite.inc"]
mod protected_macro_defsite;

emit_forbidden_audit_use_from_protected_defsite!();

fn main() {
    let _ = SqliteLogger::new();
}
