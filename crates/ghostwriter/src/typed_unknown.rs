//! Stage 2: typed unknown placeholders.
//!
//! Given detections over a (partially stage-1-substituted) text, replace
//! detected spans with typed placeholder tokens allocated from the shared
//! `PlaceholderMap`. Overlapping detections are resolved by taking the
//! first in sorted order and skipping any that start before the previous
//! end.

use crate::detect::Detection;
use crate::placeholder::PlaceholderMap;

pub fn apply(text: &str, detections: &[Detection], map: &mut PlaceholderMap) -> String {
    let mut out = String::with_capacity(text.len());
    let mut cursor: usize = 0;
    let mut last_end: usize = 0;

    for d in detections {
        if d.start < last_end {
            // overlapping with a previously applied detection — skip
            continue;
        }
        if d.start < cursor {
            // sanity guard — detections must be sorted by start
            continue;
        }
        // Append verbatim text up to the detection.
        out.push_str(&text[cursor..d.start]);
        // Allocate (or reuse) a token for this raw value.
        let token = map.intern_typed(d.kind, &d.raw);
        out.push_str(&token);
        cursor = d.end;
        last_end = d.end;
    }
    out.push_str(&text[cursor..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::placeholder::PlaceholderKind;

    fn det(start: usize, end: usize, kind: PlaceholderKind, raw: &str) -> Detection {
        Detection {
            start,
            end,
            kind,
            raw: raw.to_string(),
        }
    }

    #[test]
    fn replaces_single_email() {
        let text = "write a@x.com soon";
        let dets = vec![det(6, 13, PlaceholderKind::Email, "a@x.com")];
        let mut map = PlaceholderMap::new();
        let out = apply(text, &dets, &mut map);
        assert_eq!(out, "write <EMAIL_1> soon");
    }

    #[test]
    fn repeated_value_reuses_same_token() {
        let text = "a@x.com then a@x.com";
        let dets = vec![
            det(0, 7, PlaceholderKind::Email, "a@x.com"),
            det(13, 20, PlaceholderKind::Email, "a@x.com"),
        ];
        let mut map = PlaceholderMap::new();
        let out = apply(text, &dets, &mut map);
        assert_eq!(out, "<EMAIL_1> then <EMAIL_1>");
    }

    #[test]
    fn distinct_values_get_sequential_numbering() {
        let text = "a@x.com and b@y.com";
        let dets = vec![
            det(0, 7, PlaceholderKind::Email, "a@x.com"),
            det(12, 19, PlaceholderKind::Email, "b@y.com"),
        ];
        let mut map = PlaceholderMap::new();
        let out = apply(text, &dets, &mut map);
        assert_eq!(out, "<EMAIL_1> and <EMAIL_2>");
    }

    #[test]
    fn mixed_kinds_get_independent_counters() {
        let text = "call +49 151 1 or mail a@x.com";
        let dets = vec![
            det(5, 14, PlaceholderKind::Phone, "+49 151 1"),
            det(23, 30, PlaceholderKind::Email, "a@x.com"),
        ];
        let mut map = PlaceholderMap::new();
        let out = apply(text, &dets, &mut map);
        assert_eq!(out, "call <PHONE_1> or mail <EMAIL_1>");
    }

    #[test]
    fn overlapping_detections_later_one_is_dropped() {
        let text = "a@example.com";
        // Two overlapping detections: the full email and a sub-span.
        let dets = vec![
            det(0, 13, PlaceholderKind::Email, "a@example.com"),
            det(2, 13, PlaceholderKind::Email, "example.com"),
        ];
        let mut map = PlaceholderMap::new();
        let out = apply(text, &dets, &mut map);
        assert_eq!(out, "<EMAIL_1>");
    }

    #[test]
    fn known_placeholder_collision_reuses_semantic_token() {
        // If stage 1 already registered the customer email, stage 2
        // must reuse <CUSTOMER_EMAIL> for the same raw value.
        let text = "<CUSTOMER_EMAIL> and also m@x.com";
        let dets = vec![det(26, 33, PlaceholderKind::Email, "m@x.com")];
        let mut map = PlaceholderMap::new();
        map.insert_known("<CUSTOMER_EMAIL>", "m@x.com");
        let out = apply(text, &dets, &mut map);
        assert_eq!(out, "<CUSTOMER_EMAIL> and also <CUSTOMER_EMAIL>");
    }

    #[test]
    fn empty_detections_returns_text_unchanged() {
        let text = "nothing to see here";
        let mut map = PlaceholderMap::new();
        let out = apply(text, &[], &mut map);
        assert_eq!(out, text);
    }
}
