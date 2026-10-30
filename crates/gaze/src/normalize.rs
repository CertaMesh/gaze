pub(crate) use gaze_types::normalize::{normalize, raw_range, NormalizedText};

pub(crate) struct BoundaryView {
    pub(crate) normalized: NormalizedText,
}

/// Reinsert a boundary at each removed source run for the outbound scanner;
/// source spans retain the original UTF-8 geometry.
pub(crate) fn boundary_view(input: &str, spans: &[(usize, usize)]) -> Option<BoundaryView> {
    if !spans.windows(2).any(|pair| pair[0].1 < pair[1].0) {
        return None;
    }
    let mut text = String::with_capacity(input.len());
    let mut source = Vec::with_capacity(input.len());
    for (at, ch) in input.char_indices() {
        if at > 0 && spans[at - 1].1 < spans[at].0 {
            text.push(' ');
            source.push((spans[at - 1].1, spans[at].0));
        }
        text.push(ch);
        source.extend_from_slice(&spans[at..at + ch.len_utf8()]);
    }
    Some(BoundaryView {
        normalized: NormalizedText {
            text,
            spans: source,
        },
    })
}
