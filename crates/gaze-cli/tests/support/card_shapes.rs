// Shared by `tests/card_forward_path.rs` (no policy) and the `gaze setup` policy unit test in
// `src/commands/setup.rs` (via `include!`), so both forward-path runs pin the same shapes.

/// `(before, card, after)`: a payment card with digits touching it. The clean text must be
/// `before`, one token, then `after`: every card digit tokenized, nothing around it.
/// Solo todo 3843 and REVIEW 652 round 2 (F-A, F-B).
const CARD_SHAPES: &[(&str, &str, &str)] = &[
    // CVV or expiry after the card (todo 3843).
    ("Karte ", "4111 1111 1111 1111", " 123 (CVV)"),
    ("Karte ", "4111 1111 1111 1111", " 12 28"),
    ("Karte ", "4111111111111111", " 123"),
    ("Karte ", "4111 1111 1111 1111", " １２３"),
    // A number before the card (todo 3843; the 21-digit run the old pattern stopped short in).
    ("Nr 7 ", "4111 1111 1111 1111", ""),
    ("Nr 12345 ", "4111 1111 1111 1111", ""),
    // F-A: a separated 4+-digit prefix, a prefix before a 19-digit card, a glued 4-digit tail.
    ("2024 ", "4111 1111 1111 1111", ""),
    ("Order 5678 ", "4111 1111 1111 1111", " paid"),
    ("Ref 12 ", "4111 1111 1111 1111 003", " 45 ok"),
    ("Karte ", "4111 1111 1111 1111", "\u{200D}1234 ok"),
    ("Karte ", "4111 1111 1111 1111", "１２３４ ok"),
    // F-B: every card layout, not only 4-4-4-4, with a CVV after it.
    ("Amex ", "3782 822463 10005", " 1234"),
    ("Diners ", "3056 930902 5904", " 123"),
];

