#[path = "../audit_path_attr.inc"]
mod audit_path_attr;

fn main() {
    audit_path_attr::touch();
}
