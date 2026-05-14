use crate::ocr::OcrSpan;

/// Text and layout metadata recovered from flat OCR spans.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OrderedText {
    pub(crate) text: String,
    pub(crate) column_count: u32,
}

/// Convert flat spans into deterministic reading order.
pub(crate) fn order_spans(spans: &[OcrSpan], column_detection: bool) -> OrderedText {
    if spans.is_empty() {
        return OrderedText {
            text: String::new(),
            column_count: 1,
        };
    }

    let split_x = if column_detection && !contains_table_like_grid(spans) {
        detect_column_split(spans)
    } else {
        None
    };

    let mut left = Vec::new();
    let mut right = Vec::new();
    for span in spans.iter().filter(|span| !span.text.is_empty()) {
        if split_x
            .map(|split| span.bbox.x.saturating_add(span.bbox.w / 2) > split)
            .unwrap_or(false)
        {
            right.push(span.clone());
        } else {
            left.push(span.clone());
        }
    }

    let mut sections = Vec::new();
    let left_text = spans_to_lines(left);
    if !left_text.is_empty() {
        sections.push(left_text);
    }
    let right_text = spans_to_lines(right);
    if !right_text.is_empty() {
        sections.push(right_text);
    }

    OrderedText {
        text: sections.join("\n\n"),
        column_count: if split_x.is_some() && sections.len() > 1 {
            2
        } else {
            1
        },
    }
}

fn detect_column_split(spans: &[OcrSpan]) -> Option<u32> {
    let mut intervals = spans
        .iter()
        .filter(|span| !span.text.is_empty() && span.bbox.w > 0)
        .map(|span| (span.bbox.x, span.bbox.x.saturating_add(span.bbox.w)))
        .collect::<Vec<_>>();
    if intervals.len() < 4 {
        return None;
    }
    intervals.sort_by_key(|(left, right)| (*left, *right));

    let min_x = intervals.first().map(|(left, _)| *left)?;
    let max_x = intervals.iter().map(|(_, right)| *right).max()?;
    let page_width = max_x.saturating_sub(min_x);
    if page_width == 0 {
        return None;
    }

    let mut merged: Vec<(u32, u32)> = Vec::new();
    for (left, right) in intervals {
        match merged.last_mut() {
            Some((_, current_right)) if left <= current_right.saturating_add(24) => {
                *current_right = (*current_right).max(right);
            }
            _ => merged.push((left, right)),
        }
    }
    if merged.len() < 2 {
        return None;
    }

    let mut best_gap = 0;
    let mut best_split = None;
    for pair in merged.windows(2) {
        let left_end = pair[0].1;
        let right_start = pair[1].0;
        let gap = right_start.saturating_sub(left_end);
        if gap > best_gap {
            best_gap = gap;
            best_split = Some(left_end.saturating_add(gap / 2));
        }
    }

    let minimum_gap = (page_width / 6).max(48);
    best_split.filter(|_| best_gap >= minimum_gap)
}

fn spans_to_lines(mut spans: Vec<OcrSpan>) -> String {
    spans.sort_by_key(|span| (span.bbox.y, span.bbox.x));

    let mut lines: Vec<Vec<OcrSpan>> = Vec::new();
    for span in spans {
        let belongs_to_current_line = lines
            .last()
            .and_then(|line| line.first())
            .map(|first| span.bbox.y.abs_diff(first.bbox.y) <= span.bbox.h.max(first.bbox.h))
            .unwrap_or(false);
        if belongs_to_current_line {
            if let Some(line) = lines.last_mut() {
                line.push(span);
            }
        } else {
            lines.push(vec![span]);
        }
    }

    let mut rendered = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        if let Some(end) = table_block_end(&lines, index) {
            rendered.push(render_table_block(&lines[index..end]));
            index = end;
        } else {
            let mut line = lines[index].clone();
            line.sort_by_key(|span| span.bbox.x);
            rendered.push(
                line.into_iter()
                    .map(|span| span.text)
                    .collect::<Vec<_>>()
                    .join(" "),
            );
            index += 1;
        }
    }

    rendered.join("\n")
}

fn contains_table_like_grid(spans: &[OcrSpan]) -> bool {
    let mut spans = spans
        .iter()
        .filter(|span| !span.text.is_empty())
        .cloned()
        .collect::<Vec<_>>();
    spans.sort_by_key(|span| (span.bbox.y, span.bbox.x));
    let mut lines: Vec<Vec<OcrSpan>> = Vec::new();
    for span in spans {
        let belongs_to_current_line = lines
            .last()
            .and_then(|line| line.first())
            .map(|first| span.bbox.y.abs_diff(first.bbox.y) <= span.bbox.h.max(first.bbox.h))
            .unwrap_or(false);
        if belongs_to_current_line {
            if let Some(line) = lines.last_mut() {
                line.push(span);
            }
        } else {
            lines.push(vec![span]);
        }
    }

    (0..lines.len()).any(|index| table_block_end(&lines, index).is_some())
}

fn table_block_end(lines: &[Vec<OcrSpan>], start: usize) -> Option<usize> {
    let first = lines.get(start)?;
    if first.len() < 2 {
        return None;
    }

    let mut end = start + 1;
    while end < lines.len() && rows_align(first, &lines[end]) {
        end += 1;
    }

    if end.saturating_sub(start) >= 3 {
        Some(end)
    } else {
        None
    }
}

fn rows_align(first: &[OcrSpan], row: &[OcrSpan]) -> bool {
    if row.len() != first.len() || row.len() < 2 {
        return false;
    }

    let mut first = first.to_vec();
    let mut row = row.to_vec();
    first.sort_by_key(|span| span.bbox.x);
    row.sort_by_key(|span| span.bbox.x);
    let avg_width = first
        .iter()
        .chain(row.iter())
        .map(|span| span.bbox.w)
        .sum::<u32>()
        / (first.len() + row.len()) as u32;
    let avg_height = first
        .iter()
        .chain(row.iter())
        .map(|span| span.bbox.h)
        .sum::<u32>()
        / (first.len() + row.len()) as u32;
    let x_tolerance = (avg_width / 2).max(18);
    let width_tolerance = (avg_width / 2).max(12);
    let height_tolerance = (avg_height / 2).max(8);

    first.iter().zip(row.iter()).all(|(a, b)| {
        a.bbox.x.abs_diff(b.bbox.x) <= x_tolerance
            && a.bbox.w.abs_diff(b.bbox.w) <= width_tolerance
            && a.bbox.h.abs_diff(b.bbox.h) <= height_tolerance
    })
}

fn render_table_block(lines: &[Vec<OcrSpan>]) -> String {
    lines
        .iter()
        .map(|line| {
            let mut cells = line.clone();
            cells.sort_by_key(|span| span.bbox.x);
            cells
                .into_iter()
                .map(|span| span.text)
                .collect::<Vec<_>>()
                .join("\n")
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use crate::ocr::{BBox, OcrSpan};

    use super::*;

    fn span(text: &str, x: u32, y: u32) -> OcrSpan {
        OcrSpan {
            text: text.to_string(),
            bbox: BBox { x, y, w: 60, h: 16 },
            confidence: Some(0.9),
        }
    }

    #[test]
    fn column_detection_orders_left_column_before_right_column() {
        let spans = vec![
            span("left-1", 20, 10),
            span("right-1", 320, 10),
            span("left-2", 20, 40),
            span("right-2", 320, 40),
        ];

        let ordered = order_spans(&spans, true);

        assert_eq!(ordered.column_count, 2);
        assert_eq!(ordered.text, "left-1\nleft-2\n\nright-1\nright-2");
    }

    #[test]
    fn column_detection_can_be_disabled() {
        let spans = vec![
            span("left-1", 20, 10),
            span("right-1", 320, 10),
            span("left-2", 20, 40),
            span("right-2", 320, 40),
        ];

        let ordered = order_spans(&spans, false);

        assert_eq!(ordered.column_count, 1);
        assert_eq!(ordered.text, "left-1 right-1\nleft-2 right-2");
    }

    #[test]
    fn table_like_grid_preserves_cell_boundaries_inline() {
        let spans = vec![
            span("Name", 20, 10),
            span("Email", 140, 10),
            span("Jane", 20, 40),
            span("alice@example.invalid", 140, 40),
            span("Alex", 20, 70),
            span("alex@example.invalid", 140, 70),
        ];

        let ordered = order_spans(&spans, true);

        assert_eq!(ordered.column_count, 1);
        assert_eq!(
            ordered.text,
            "Name\nEmail\n\nJane\nalice@example.invalid\n\nAlex\nalex@example.invalid"
        );
    }
}
