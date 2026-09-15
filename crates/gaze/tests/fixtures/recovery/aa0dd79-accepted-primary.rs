use gaze::*;
struct Split;
impl Detector for Split {
 fn detect(&self,input:&str)->Vec<Detection> {
 assert_eq!(input,"\u{0308}\u{0301}");
 vec![Detection::new(0..2,PiiClass::Name,"a"),Detection::new(2..4,PiiClass::Name,"b")]
 }
}
#[test]
fn report_exact_primary_raw_scalar_collision() {
 let raw="\u{0344}";let session=Session::new(Scope::Ephemeral).unwrap();
 let pipeline=Pipeline::builder().detector(Split).rule(DefaultRule::new(Action::Tokenize)).build().unwrap();
 let result=pipeline.clean_with_safety_net_policy_detect_context(&session,RawDocument::Text(raw.into()),&[LocaleTag::Global],&DictionaryBundle::default(),SafetyNetPolicy::default());
 match result {
 Ok((CleanDocument::Text(clean),spans,_))=>{
 println!("ACCEPTED spans={spans:?} restore={:?}",session.restore_strict_text(&clean));
 assert_eq!(spans.iter().map(|s|s.raw_span.clone()).collect::<Vec<_>>(),vec![0..2,0..2]);
 assert_eq!(session.restore_strict_text(&clean).unwrap(),"\u{0344}\u{0344}");
 },other=>panic!("unexpected {other:?}"),
 }
}
