//! Regex matching with optional provenance boundaries between joined characters.
//!
//! The canonical input stays joined. A removed source run is represented only
//! inside this matcher by WORD JOINER (which normalization has already removed).
//! Consuming atoms can skip it; assertions and delimiter classes can observe it.
//! Thus an outer boundary and arbitrarily many inner gaps coexist in one match.
use regex::Regex;
use regex_syntax::ast::{parse::Parser, Ast};
use std::ops::Range;

pub(super) const MARKER: char = '\u{2060}';
// Accepted original patterns use regex's 10 MiB cap; boundary expansion gets
// the same bounded eightfold allowance enforced by the differential suite.
const COMPILED_SIZE_LIMIT: usize = 8 * 10 * (1 << 20);

// Dot and broad classes retain their consuming-atom cardinality.
fn normalize_literals(ast: &mut Ast, marker: bool) -> Result<(), regex::Error> {
    use regex_syntax::ast::{ClassSet, ClassSetItem};
    fn named(set: &ClassSet) -> bool {
        match set {
            ClassSet::Item(item) => match item {
                ClassSetItem::Literal(literal) => ignorable(literal.c),
                ClassSetItem::Range(range) => ignorable_range(range.start.c, range.end.c),
                ClassSetItem::Union(union) => union
                    .items
                    .iter()
                    .any(|item| named(&ClassSet::Item(item.clone()))),
                ClassSetItem::Bracketed(class) => !class.negated && named(&class.kind),
                _ => false,
            },
            ClassSet::BinaryOp(op) => named(&op.lhs) || named(&op.rhs),
        }
    }
    let replacement = match ast {
        Ast::Literal(literal) if ignorable(literal.c) => Some(if marker {
            r"(?u:\u{2060})*".to_owned()
        } else {
            "(?:)".to_owned()
        }),
        Ast::ClassBracketed(class)
            if !class.negated
                && named(&class.kind)
                && class_accepts_ignorable(&ast.to_string()) =>
        {
            Some(if marker {
                format!(r"(?:{}|(?u:\u{{2060}})*)", ast)
            } else {
                format!("(?:{}|(?:))", ast)
            })
        }
        Ast::Group(group) => {
            normalize_literals(&mut group.ast, marker)?;
            None
        }
        Ast::Repetition(repetition) => {
            normalize_literals(&mut repetition.ast, marker)?;
            None
        }
        Ast::Concat(concat) => {
            for child in &mut concat.asts {
                normalize_literals(child, marker)?;
            }
            None
        }
        Ast::Alternation(alternation) => {
            for child in &mut alternation.asts {
                normalize_literals(child, marker)?;
            }
            None
        }
        _ => None,
    };
    if let Some(replacement) = replacement {
        *ast = Parser::new()
            .parse(&replacement)
            .map_err(|err| regex::Error::Syntax(err.to_string()))?;
    }
    Ok(())
}

fn ignorable_ranges() -> &'static regex_syntax::hir::ClassUnicode {
    static RANGES: std::sync::OnceLock<regex_syntax::hir::ClassUnicode> =
        std::sync::OnceLock::new();
    RANGES.get_or_init(|| {
        let hir = regex_syntax::Parser::new()
            .parse(r"\p{Default_Ignorable_Code_Point}")
            .expect("Unicode property is supported");
        let regex_syntax::hir::HirKind::Class(regex_syntax::hir::Class::Unicode(class)) =
            hir.kind()
        else {
            unreachable!("Unicode property is a class")
        };
        class.clone()
    })
}

fn ignorable_range(start: char, end: char) -> bool {
    ignorable_ranges()
        .iter()
        .any(|range| range.start() <= end && range.end() >= start)
}

fn ignorable(ch: char) -> bool {
    ignorable_range(ch, ch)
}

fn class_accepts_ignorable(pattern: &str) -> bool {
    let Ok(hir) = regex_syntax::Parser::new().parse(pattern) else {
        return false;
    };
    match hir.kind() {
        regex_syntax::hir::HirKind::Literal(literal) => {
            std::str::from_utf8(&literal.0).is_ok_and(|text| text.chars().any(ignorable))
        }
        regex_syntax::hir::HirKind::Class(regex_syntax::hir::Class::Unicode(class)) => class
            .iter()
            .any(|range| ignorable_range(range.start(), range.end())),
        _ => false,
    }
}

pub(super) fn has_ignorable_literals(pattern: &str) -> Result<bool, regex::Error> {
    let mut ast = Parser::new()
        .parse(pattern)
        .map_err(|err| regex::Error::Syntax(err.to_string()))?;
    let original = ast.to_string();
    normalize_literals(&mut ast, false)?;
    Ok(ast.to_string() != original)
}

pub(super) fn joined_pattern(pattern: &str) -> Result<String, regex::Error> {
    let mut ast = Parser::new()
        .parse(pattern)
        .map_err(|err| regex::Error::Syntax(err.to_string()))?;
    normalize_literals(&mut ast, false)?;
    Ok(ast.to_string())
}

pub(super) fn compile(pattern: &str) -> Result<Regex, regex::Error> {
    #[cfg(feature = "test-support")]
    let started = std::time::Instant::now();
    fn augment(ast: &mut Ast) -> Result<(), regex::Error> {
        match ast {
            Ast::Literal(_)
            | Ast::Dot(_)
            | Ast::ClassUnicode(_)
            | Ast::ClassPerl(_)
            | Ast::ClassBracketed(_) => {
                *ast = Parser::new()
                    .parse(&format!(r"(?:{}(?u:\u{{2060}})*)", ast))
                    .map_err(|err| regex::Error::Syntax(err.to_string()))?;
            }
            Ast::Repetition(repetition) => augment(&mut repetition.ast)?,
            Ast::Group(group) => augment(&mut group.ast)?,
            Ast::Concat(concat) => {
                for child in &mut concat.asts {
                    augment(child)?;
                }
            }
            Ast::Alternation(alternation) => {
                for child in &mut alternation.asts {
                    augment(child)?;
                }
            }
            Ast::Empty(_) | Ast::Flags(_) | Ast::Assertion(_) => {}
        }
        Ok(())
    }
    let mut ast = Parser::new()
        .parse(pattern)
        .map_err(|err| regex::Error::Syntax(err.to_string()))?;
    normalize_literals(&mut ast, true)?;
    augment(&mut ast)?;
    let regex = regex::RegexBuilder::new(&ast.to_string())
        .size_limit(COMPILED_SIZE_LIMIT)
        .build()?;
    #[cfg(feature = "test-support")]
    {
        let elapsed = started.elapsed().as_nanos() as u64;
        COMPILED.with(|compiled| {
            compiled
                .borrow_mut()
                .push((pattern.to_owned(), regex.as_str().to_owned(), elapsed))
        });
    }
    Ok(regex)
}

pub(super) fn is_gap(spans: Option<&[(usize, usize)]>, at: usize) -> bool {
    at > 0
        && spans.is_some_and(|spans| {
            spans
                .get(at)
                .zip(spans.get(at - 1))
                .is_some_and(|(right, left)| left.1 < right.0)
        })
}

pub(super) struct View {
    pub(super) text: String,
    to_joined: Vec<usize>,
    to_marked: Vec<usize>,
}

impl View {
    pub(super) fn new(input: &str, spans: Option<&[(usize, usize)]>) -> Option<Self> {
        Self::with_boundary(input, spans, MARKER)
    }

    pub(super) fn with_boundary(
        input: &str,
        spans: Option<&[(usize, usize)]>,
        boundary: char,
    ) -> Option<Self> {
        let spans = spans?;
        if !spans.windows(2).any(|pair| pair[0].1 < pair[1].0) {
            return None;
        }
        let mut text = String::with_capacity(input.len());
        let mut to_joined = vec![0];
        let mut to_marked = vec![0; input.len() + 1];
        for (at, ch) in input.char_indices() {
            to_marked[at] = text.len();
            if is_gap(Some(spans), at) {
                text.push(boundary);
                to_joined.extend(std::iter::repeat_n(at, boundary.len_utf8()));
            }
            let start = text.len();
            text.push(ch);
            for byte in 1..=ch.len_utf8() {
                to_joined.push(at + byte);
                to_marked[at + byte] = start + byte;
            }
        }
        Some(Self {
            text,
            to_joined,
            to_marked,
        })
    }

    pub(super) fn joined(&self, range: Range<usize>) -> Range<usize> {
        self.to_joined[range.start]..self.to_joined[range.end]
    }

    pub(super) fn marked(&self, at: usize) -> usize {
        self.to_marked[at]
    }
}

#[cfg(feature = "test-support")]
thread_local! {
    static COMPILED: std::cell::RefCell<Vec<(String, String, u64)>> = const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(test)]
#[path = "source_regex_tests.rs"]
mod tests;

#[cfg(feature = "test-support")]
pub(crate) fn take_compilations() -> Vec<(String, String, u64)> {
    COMPILED.with(|compiled| std::mem::take(&mut *compiled.borrow_mut()))
}
