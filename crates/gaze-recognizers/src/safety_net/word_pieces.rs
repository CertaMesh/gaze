//! Word grouping shared by every model safety-net decoder.

use gaze_types::is_inside_word;

/// Groups piece indices into words. A model labels sub-word pieces, but a span
/// must never start or end inside a word, so the word is the smallest unit a
/// span may cover. Two pieces belong to one word when they touch and the seam
/// between them is inside a word ([`is_inside_word`], the same rule the
/// pipeline's sub-word guard uses). Offsets are byte offsets into `source`; the
/// rule reads the source text rather than tokenizer markers (`##`, `▁`) so
/// every backend gets the same boundaries. Empty pieces are skipped.
pub(crate) fn group_word_pieces(source: &str, piece_spans: &[(usize, usize)]) -> Vec<Vec<usize>> {
    let mut words: Vec<Vec<usize>> = Vec::new();
    let mut previous_end = None;
    for (index, &(start, end)) in piece_spans.iter().enumerate() {
        if start >= end {
            continue;
        }
        let joins_previous = previous_end == Some(start) && is_inside_word(source, start);
        match words.last_mut() {
            Some(word) if joins_previous => word.push(index),
            _ => words.push(vec![index]),
        }
        previous_end = Some(end);
    }
    words
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn touching_pieces_join_only_across_an_alphanumeric_seam() {
        // "ab12-cd ef": ab|12 join, 12|- and -|cd do not, cd|<space> gap, ef alone.
        let text = "ab12-cd ef";
        let spans = [(0, 2), (2, 4), (4, 5), (5, 7), (8, 10)];
        assert_eq!(
            group_word_pieces(text, &spans),
            vec![vec![0, 1], vec![2], vec![3], vec![4]]
        );
    }

    #[test]
    fn multibyte_seams_use_character_classes() {
        // "Grüße": Gr|üße joins across `r|ü`; offsets are bytes.
        let text = "Grüße 7";
        assert_eq!(
            group_word_pieces(text, &[(0, 2), (2, 8), (9, 10)]),
            vec![vec![0, 1], vec![2]]
        );
    }
}
