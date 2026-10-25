#![cfg(feature = "os-keychain")]

use std::any::Any;
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};

use gaze_token_bridge::persistent::{FileCorpusIndexStore, DEFAULT_DOMAIN_ID, INDEX_FILE_NAME};
use gaze_token_bridge::util::sha256_hex;
use keyring::credential::{Credential, CredentialBuilderApi};

static BACKEND_CALLS: AtomicUsize = AtomicUsize::new(0);

// Install before touching the index, so even a broken isolation guard cannot
// contact the real OS store. Mutation probes must retain this fake.
struct ForbiddenBackend;

impl CredentialBuilderApi for ForbiddenBackend {
    fn build(&self, _: Option<&str>, _: &str, _: &str) -> keyring::Result<Box<Credential>> {
        BACKEND_CALLS.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(keyring::mock::MockCredential::default()))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[test]
fn cargo_tests_and_children_never_enter_the_os_keystore() {
    keyring::set_default_credential_builder(Box::new(ForbiddenBackend));
    assert!(std::env::var_os("GAZE_DISABLE_OS_KEYSTORE").is_some());
    std::env::remove_var("GAZE_INDEX_KEY");
    let dir = std::env::temp_dir().join(format!("gaze-keystore-{}", std::process::id()));
    fs::create_dir(&dir).expect("isolated index directory");
    let store = FileCorpusIndexStore::load_or_create(&dir, DEFAULT_DOMAIN_ID, &[]).unwrap();
    let saved = store.save();
    assert_eq!(
        BACKEND_CALLS.load(Ordering::SeqCst),
        0,
        "native backend reached on save"
    );
    assert!(format!("{:?}", saved.unwrap_err()).contains("OS key store disabled"));
    assert!(!dir.join(INDEX_FILE_NAME).exists());

    // A structurally valid keychain envelope reaches the read guard without
    // creating a credential. Its payload is synthetic and never decrypted.
    let account = format!(
        "owner-index-v1:{}",
        sha256_hex(dir.canonicalize().unwrap().to_string_lossy().as_ref())
    );
    let mut envelope = b"GAZEIDX1".to_vec();
    envelope.extend_from_slice(&[1, 2]);
    envelope.extend_from_slice(&u16::try_from(account.len()).unwrap().to_be_bytes());
    envelope.extend_from_slice(account.as_bytes());
    envelope.extend_from_slice(&[0; 16 + 12 + 16]);
    fs::write(dir.join(INDEX_FILE_NAME), envelope).unwrap();
    let loaded = FileCorpusIndexStore::load(&dir);
    assert_eq!(
        BACKEND_CALLS.load(Ordering::SeqCst),
        0,
        "native backend reached on load"
    );
    assert!(format!("{:?}", loaded.unwrap_err()).contains("OS key store disabled"));
    fs::remove_dir_all(dir).unwrap();
    if std::env::var_os("GAZE_KEYSTORE_PROBE_CHILD").is_none() {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "cargo_tests_and_children_never_enter_the_os_keystore",
                "--nocapture",
            ])
            .env("GAZE_KEYSTORE_PROBE_CHILD", "1")
            .output()
            .expect("run child test binary");
        assert!(
            child.status.success(),
            "{}",
            String::from_utf8_lossy(&child.stdout)
        );
    }
}
