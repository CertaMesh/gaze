use gaze::{DictionaryBundle, LocaleTag, Pipeline, RawDocument, Scope, Session};

fn main() {
    let pipeline = Pipeline::builder().build().expect("pipeline");
    let session = Session::new(Scope::Ephemeral).expect("session");
    let mut snapshot = session
        .begin_transaction()
        .commit()
        .expect("committed snapshot");
    let _ = pipeline.pseudonymize_transaction_with_detect_context(
        &mut snapshot,
        RawDocument::Text("synthetic text".to_string()),
        &[LocaleTag::Global],
        &DictionaryBundle::default(),
    );
}
