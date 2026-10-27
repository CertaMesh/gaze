//! Lexical tag syntax shared by primary record ownership and repeat matching.
use std::{collections::HashMap, ops::Range};

#[derive(Default)]
pub struct MarkupRegions {
    assignments: Vec<Range<usize>>,
    syntax: Vec<Range<usize>>,
}

impl MarkupRegions {
    pub fn new(text: &str) -> Self {
        if !text.contains('<') {
            return Self::default();
        }
        let data = data_scalars(text);
        let bytes = text.as_bytes();
        let closers = closing_tags(text);
        let mut result = Self::default();
        let mut at = 0;
        while at < bytes.len() {
            let data_at = data.partition_point(|range| range.end <= at);
            if let Some(range) = data.get(data_at).filter(|range| range.contains(&at)) {
                at = range.end;
                continue;
            }
            if bytes[at] != b'<' {
                at += 1;
                continue;
            }
            if let Some((end, syntax)) = assignment_tag(text, at, &closers) {
                result.assignments.push(at..end);
                result.syntax.extend(syntax);
                at = end;
            } else {
                at += 1;
            }
        }
        result
    }

    pub fn contains_assignment(&self, at: usize) -> bool {
        let index = self.assignments.partition_point(|range| range.end <= at);
        self.assignments
            .get(index)
            .is_some_and(|range| range.contains(&at))
    }

    pub fn blocks_repeat(&self, span: &Range<usize>) -> bool {
        let index = self.syntax.partition_point(|range| range.end <= span.start);
        self.syntax
            .get(index)
            .is_some_and(|range| range.start <= span.start && span.end <= range.end)
    }
}

fn assignment_tag(text: &str, start: usize, closers: &HashMap<String, usize>) -> Option<(usize, Vec<Range<usize>>)> {
    const MAX_TAG_BYTES: usize = 2048;
    let bytes = text.as_bytes();
    let limit = start.saturating_add(MAX_TAG_BYTES).min(bytes.len());
    let mut at = start + 1;
    if bytes.get(at) == Some(&b'/') {
        return None;
    }
    let name_start = |byte: u8| byte.is_ascii_alphabetic() || byte == b'_';
    let name_char = |byte: u8| name_start(byte) || byte.is_ascii_digit() || byte == b'-';
    if !bytes.get(at).copied().is_some_and(name_start) {
        return None;
    }
    while at < limit && name_char(bytes[at]) {
        at += 1;
    }
    let tag = &text[start+1..at];
    let folded_tag = tag.to_ascii_lowercase();
    let known_tag = known_tag(&folded_tag) || tag.contains('-');
    let component = tag.as_bytes()[0].is_ascii_uppercase();
    let mut html_attributes = true;
    let mut lowercase_attributes = true;
    let mut syntax_start = start;
    let mut syntax = Vec::new();
    let mut assignment = false;
    while at < limit {
        if bytes[at] == b'>' || bytes.get(at..at + 2) == Some(b"/>") {
            let self_closing = bytes[at] == b'/';
            let compact_end = at > start && !bytes[at-1].is_ascii_whitespace();
            at += if bytes[at] == b'>' { 1 } else { 2 };
            let paired = self_closing || closers.get(&folded_tag).is_some_and(|&close| close >= at);
            let fragment = lowercase_attributes && compact_end
                && bytes.get(at).is_none_or(|byte| byte.is_ascii_whitespace() || *byte == b'<');
            let established = (known_tag && (paired || html_attributes || fragment)) || (component && paired);
            if at > limit || !assignment || !established {
                return None;
            }
            syntax.push(syntax_start..at);
            return Some((at, syntax));
        }
        if !bytes[at].is_ascii_whitespace() {
            return None;
        }
        while at < limit && bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        if at == limit {
            return None;
        }
        if bytes[at] == b'>' || bytes.get(at..at + 2) == Some(b"/>") {
            continue;
        }
        let attribute_start = at;
        let attribute_char = |byte: u8| name_char(byte) || b":@.".contains(&byte);
        if !attribute_char(bytes[at]) { return None; }
        while at < limit && attribute_char(bytes[at]) { at += 1; }
        let attribute = &text[attribute_start..at];
        html_attributes &= html_attribute(attribute);
        lowercase_attributes &= !attribute.bytes().any(|byte| byte.is_ascii_uppercase());
        if at == limit {
            return None;
        }
        if bytes[at] != b'=' {
            continue;
        }
        assignment = true;
        at += 1;
        if at == limit {
            return None;
        }
        if matches!(bytes[at], b'"' | b'\'') {
            let quote = bytes[at];
            syntax.push(syntax_start..at + 1);
            at += 1;
            while at < limit && bytes[at] != quote && bytes[at] != b'<' {
                at += 1;
            }
            if at == limit || bytes[at] != quote {
                return None;
            }
            syntax_start = at;
            at += 1;
        } else {
            let value_start = at;
            if bytes[at] == b'=' { return None; }
            while at < limit && !bytes[at].is_ascii_whitespace() && bytes[at] != b'>' {
                if bytes.get(at..at + 2) == Some(b"/>") {
                    break;
                }
                if b"<\"'`".contains(&bytes[at]) {
                    return None;
                }
                at += 1;
            }
            if at == value_start {
                return None;
            }
        }
    }
    None
}

fn closing_tags(text: &str) -> HashMap<String, usize> {
    let mut tags = HashMap::new();
    for (start, _) in text.match_indices("</") {
        let tail = &text[start + 2..];
        let end = tail.find(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-').unwrap_or(tail.len());
        if end > 0 && tail[end..].trim_start_matches([' ', '\t', '\r', '\n']).starts_with('>') {
            tags.insert(tail[..end].to_ascii_lowercase(), start);
        }
    }
    tags
}

fn known_tag(tag: &str) -> bool {
    matches!(tag,
        "a" | "abbr" | "address" | "area" | "article" | "aside" | "audio" | "b" | "base"
        | "bdi" | "bdo" | "blockquote" | "body" | "br" | "button" | "canvas" | "caption"
        | "cite" | "code" | "col" | "colgroup" | "data" | "datalist" | "dd" | "del" | "details"
        | "dfn" | "dialog" | "div" | "dl" | "dt" | "em" | "embed" | "fieldset" | "figcaption"
        | "figure" | "footer" | "form" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "head"
        | "header" | "hgroup" | "hr" | "html" | "i" | "iframe" | "img" | "input" | "ins"
        | "kbd" | "label" | "legend" | "li" | "link" | "main" | "map" | "mark" | "menu"
        | "meta" | "meter" | "nav" | "noscript" | "object" | "ol" | "optgroup" | "option"
        | "output" | "p" | "picture" | "pre" | "progress" | "q" | "rp" | "rt" | "ruby"
        | "s" | "samp" | "script" | "search" | "section" | "select" | "slot" | "small"
        | "source" | "span" | "strong" | "style" | "sub" | "summary" | "sup" | "table"
        | "tbody" | "td" | "template" | "textarea" | "tfoot" | "th" | "thead" | "time"
        | "title" | "tr" | "track" | "u" | "ul" | "var" | "video" | "wbr"
        | "svg" | "g" | "path" | "circle" | "ellipse" | "line" | "polyline" | "polygon"
        | "rect" | "text" | "tspan" | "textpath" | "defs" | "symbol" | "use" | "image"
        | "foreignobject" | "clippath" | "mask" | "pattern" | "marker" | "lineargradient"
        | "radialgradient" | "stop" | "filter" | "fegaussianblur" | "animate" | "set"
        | "math" | "mi" | "mn" | "mo" | "mrow" | "mfrac" | "msqrt" | "mroot" | "msub"
        | "msup" | "msubsup" | "munder" | "mover" | "munderover" | "mtable" | "mtr"
        | "mtd" | "mtext" | "mspace" | "semantics" | "annotation" | "annotation-xml")
}

fn html_attribute(attribute: &str) -> bool {
    let attribute = attribute.to_ascii_lowercase();
    attribute.starts_with([':', '@'])
        || ["data-", "aria-", "on", "v-", "x-", "ng-", "xml:", "xmlns"].iter().any(|prefix| attribute.starts_with(prefix))
        || matches!(attribute.as_str(),
            "id" | "class" | "style" | "title" | "lang" | "dir" | "hidden" | "role" | "tabindex"
            | "accesskey" | "contenteditable" | "draggable" | "spellcheck" | "translate" | "inert"
            | "href" | "src" | "alt" | "width" | "height" | "name" | "value" | "type" | "disabled"
            | "checked" | "selected" | "multiple" | "readonly" | "required" | "placeholder" | "for"
            | "action" | "method" | "target" | "rel" | "download" | "content" | "charset" | "http-equiv"
            | "media" | "sizes" | "srcset" | "loading" | "async" | "defer" | "integrity" | "crossorigin"
            | "colspan" | "rowspan" | "scope" | "headers" | "min" | "max" | "step" | "pattern"
            | "autocomplete" | "autofocus" | "accept" | "enctype" | "form" | "list" | "size" | "maxlength"
            | "minlength" | "rows" | "cols" | "wrap" | "open" | "datetime" | "cite" | "start" | "reversed"
            | "controls" | "autoplay" | "loop" | "muted" | "poster" | "preload" | "kind" | "label"
            | "viewbox" | "fill" | "stroke" | "d" | "cx" | "cy" | "r" | "x" | "y" | "transform")
}

fn data_scalars(text: &str) -> Vec<Range<usize>> {
    let regions = crate::json_regions::json_regions(text);
    let mut data: Vec<_> = crate::json_regions::json_strings(text, &regions)
        .into_iter()
        .filter_map(|(span, key)| (!key).then_some(span))
        .collect();
    let trimmed = text.trim();
    if serde_json::from_str::<String>(trimmed).is_ok() {
        let start = text.len() - text.trim_start().len();
        data.push(start + 1..start + trimmed.len() - 1);
    }
    if text.contains('\\') {
        let mut view = text.to_owned();
        let mut source: Vec<_> = (0..text.len()).map(|at| at..at + 1).collect();
        for _ in 0..32 {
            let Some((decoded, projection)) = crate::json_regions::unescape_json_fragments(&view)
            else {
                break;
            };
            source = projection
                .into_iter()
                .map(|range| source[range.start].start..source[range.end - 1].end)
                .collect();
            view = decoded;
            let regions = crate::json_regions::json_regions(&view);
            for (range, key) in crate::json_regions::json_strings(&view, &regions) {
                if !key && !range.is_empty() {
                    data.push(source[range.start].start..source[range.end - 1].end);
                }
            }
        }
    }
    let bytes = text.as_bytes();
    // Closed CSV cells. A quoted cell must start and end at a record delimiter.
    let mut at = 0;
    while at < bytes.len() {
        while at < bytes.len() && matches!(bytes[at], b' ' | b'\t') {
            at += 1;
        }
        if bytes.get(at) == Some(&b'"') {
            let opening = at;
            at = quote_end(bytes, at, b'"', false);
            if at < bytes.len() {
                let suffix = text[at + 1..].trim_start_matches([' ', '\t']);
                if suffix
                    .chars()
                    .next()
                    .is_none_or(|ch| matches!(ch, ',' | '\r' | '\n'))
                {
                    data.push(opening + 1..at);
                }
                at += 1;
            }
        }
        while at < bytes.len() && !matches!(bytes[at], b',' | b'\r' | b'\n') {
            at += 1;
        }
        at += 1;
    }
    // SQL record prefixes establish literal context. Quotes in prose do not.
    let mut at = 0;
    let mut line_start = true;
    let mut sql = false;
    while at < bytes.len() {
        if line_start && !sql {
            let tail = text[at..].trim_start_matches([' ', '\t']);
            sql = ["select", "insert", "update", "replace", "values"]
                .iter()
                .any(|word| {
                    tail.get(..word.len())
                        .is_some_and(|head| head.eq_ignore_ascii_case(word))
                        && tail[word.len()..].chars().next().is_none_or(|ch| {
                            ch.is_ascii() && !ch.is_ascii_alphanumeric() && ch != '_'
                        })
                });
        }
        line_start = false;
        if sql && bytes.get(at..at + 2) == Some(b"--") {
            let start = at;
            while at < bytes.len() && !matches!(bytes[at], b'\r' | b'\n') {
                at += 1;
            }
            data.push(start..at);
            line_start = true;
        } else if sql && bytes.get(at..at + 2) == Some(b"/*") {
            let start = at;
            at += 2;
            while at + 1 < bytes.len() && &bytes[at..at + 2] != b"*/" {
                at += 1;
            }
            at = (at + 2).min(bytes.len());
            data.push(start..at);
        } else if sql && matches!(bytes[at], b'\'' | b'"' | b'`') {
            let opening = at;
            at = quote_end(bytes, at, bytes[at], true);
            if at < bytes.len() {
                data.push(opening + 1..at);
                at += 1;
            }
        } else {
            if matches!(bytes[at], b'\r' | b'\n') {
                line_start = true;
            }
            at += 1;
        }
    }
    data.retain(|range| !range.is_empty());
    data.sort_unstable_by_key(|range| range.start);
    let mut merged: Vec<Range<usize>> = Vec::new();
    for range in data {
        if let Some(last) = merged.last_mut().filter(|last| last.end >= range.start) {
            last.end = last.end.max(range.end);
        } else {
            merged.push(range);
        }
    }
    merged
}

fn quote_end(bytes: &[u8], opening: usize, quote: u8, escapes: bool) -> usize {
    let mut at = opening + 1;
    while at < bytes.len() {
        if escapes && bytes[at] == b'\\' {
            at = (at + 2).min(bytes.len());
        } else if bytes[at] == quote {
            if bytes.get(at + 1) != Some(&quote) {
                break;
            }
            at += 2;
        } else {
            at += 1;
        }
    }
    at
}
