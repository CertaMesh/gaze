use gaze::{EmptyCustomClassName, PiiClass, Scope, Session};
use gaze_token_bridge::util::parse_token_class;

#[test]
fn every_mintable_class_round_trips_and_empty_custom_names_are_rejected() {
    assert_eq!(PiiClass::custom("!!!"), Err(EmptyCustomClassName));
    let session = Session::new(Scope::Ephemeral).unwrap();
    let mut staged = session.begin_transaction();
    for class in PiiClass::builtin_variants().iter().cloned().chain([
        PiiClass::custom("Case-Reference").unwrap(),
        PiiClass::family("us-9-digit-id"),
    ]) {
        let token = session.tokenize(&class, "synthetic value").unwrap();
        assert_eq!(parse_token_class(&token), Some(class.clone()));
        assert_eq!(session.restore_strict(&token).unwrap(), "synthetic value");
        let token = staged.tokenize(&class, "synthetic value").unwrap();
        assert_eq!(parse_token_class(&token), Some(class));
        assert_eq!(staged.restore_strict(&token).unwrap(), "synthetic value");
    }
}
