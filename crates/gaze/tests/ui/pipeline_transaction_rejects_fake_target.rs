use gaze::{DictionaryBundle, LocaleTag, Pipeline, RawDocument};

struct FakeTarget;

fn main() {
    let pipeline = Pipeline::builder().build().expect("pipeline");
    let mut fake = FakeTarget;
    let _ = pipeline.pseudonymize_transaction_with_detect_context(
        &mut fake,
        RawDocument::Text("synthetic text".to_string()),
        &[LocaleTag::Global],
        &DictionaryBundle::default(),
    );
}
