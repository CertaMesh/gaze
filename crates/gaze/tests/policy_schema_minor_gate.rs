use gaze::{Policy, PolicyError};

#[test]
fn policy_schema_minor_gate_rejects_two_digit_minors() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("policy.toml");
    for version in ["0.1.0", "0.1.7", "0.1.99", "0.10.0", "0.19.0", "0.2.0"] {
        std::fs::write(
            &path,
            format!(
                "schema_version = \"{version}\"\n[session]\nscope = \"ephemeral\"\n[[rule]]\nkind = \"default\"\naction = \"preserve\"\n"
            ),
        )
        .unwrap();
        match version {
            "0.1.0" | "0.1.7" | "0.1.99" => {
                assert_eq!(Policy::load(&path).unwrap().schema_version, version);
            }
            _ => assert!(matches!(
                Policy::load(&path),
                Err(PolicyError::PolicySchemaUnsupported { found, supported })
                    if found == version && supported == "0.1."
            )),
        }
    }
}
