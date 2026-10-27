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
    augment(&mut ast)?;
    let regex = Regex::new(&ast.to_string())?;
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
