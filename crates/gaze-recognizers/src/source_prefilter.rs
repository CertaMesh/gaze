//! Necessary literal evidence only: an unknown plan always permits detection.
use regex_syntax::hir::{literal::Extractor, Hir, HirKind};

pub(super) enum Necessary {
    Unknown,
    Prefixes(Vec<Vec<u8>>),
    Delimited(regex::Regex),
    All(Vec<Self>),
    Any(Vec<Self>),
}

impl Necessary {
    pub(super) fn new(pattern: &str) -> Self {
        let Ok(pattern) = crate::source_regex::joined_pattern(pattern) else {
            return Self::Unknown;
        };
        regex_syntax::Parser::new()
            .parse(&pattern)
            .map_or(Self::Unknown, |hir| Self::from_hir(&hir))
    }

    fn all(plans: impl IntoIterator<Item = Self>) -> Self {
        let mut flattened = Vec::new();
        for plan in plans {
            match plan {
                Self::Unknown => {}
                Self::All(children) => flattened.extend(children),
                other => flattened.push(other),
            }
        }
        let mut plans = flattened;
        match plans.len() {
            0 => Self::Unknown,
            1 => plans.pop().expect("one necessary plan"),
            _ => Self::All(plans),
        }
    }

    fn marker_free(prefix: &[u8]) -> bool {
        let mut buffer = [0; 4];
        let marker = crate::source_regex::MARKER
            .encode_utf8(&mut buffer)
            .as_bytes();
        !prefix.windows(marker.len()).any(|bytes| bytes == marker)
            && !(1..marker.len()).any(|len| prefix.ends_with(&marker[..len]))
    }

    fn from_hir(hir: &Hir) -> Self {
        if let HirKind::Capture(capture) = hir.kind() {
            return Self::from_hir(&capture.sub);
        }
        // HIR already expands each scoped case flag, including Unicode folds.
        // A finite prefix that cannot consume the provenance marker must occur
        // in the JOINED input, even across raw gaps. Marker-bearing or truncated
        // marker prefixes stay unknown, as do empty and infinite sequences.
        let sequence = Extractor::new().extract(hir);
        let prefix = match sequence.literals() {
            Some(literals)
                if !literals.is_empty()
                    && literals.iter().all(|literal| {
                        !literal.as_bytes().is_empty() && Self::marker_free(literal.as_bytes())
                    }) =>
            {
                Self::Prefixes(
                    literals
                        .iter()
                        .map(|literal| literal.as_bytes().to_vec())
                        .collect(),
                )
            }
            _ => Self::Unknown,
        };
        if sequence.is_exact() && !matches!(prefix, Self::Unknown) {
            return prefix;
        }
        let inner = match hir.kind() {
            HirKind::Concat(parts) => Self::all(
                parts.iter().map(Self::from_hir).chain(
                    parts
                        .windows(2)
                        .filter_map(|pair| Self::delimited_literal(&pair[0], &pair[1])),
                ),
            ),
            HirKind::Alternation(parts) => {
                let plans = parts.iter().map(Self::from_hir).collect::<Vec<_>>();
                if plans.iter().any(|plan| matches!(plan, Self::Unknown)) {
                    Self::Unknown
                } else {
                    Self::Any(plans)
                }
            }
            HirKind::Capture(capture) => Self::from_hir(&capture.sub),
            HirKind::Repetition(repetition) if repetition.min > 0 => {
                Self::from_hir(&repetition.sub)
            }
            _ => Self::Unknown,
        };
        Self::all([prefix, inner])
    }

    // Keep the adjacency in e.g. (?:\A|[\s(])@. An email's interior @ is
    // not evidence for a handle. Absolute start survives joining, as does an
    // adjacent source character unless the separator class can eat the marker.
    fn delimited_literal(left: &Hir, right: &Hir) -> Option<Self> {
        use regex_syntax::hir::{Class, Look};
        let HirKind::Alternation(branches) = left.kind() else {
            return None;
        };
        let HirKind::Literal(literal) = right.kind() else {
            return None;
        };
        if !Self::marker_free(&literal.0)
            || !branches
                .iter()
                .any(|b| matches!(b.kind(), HirKind::Look(Look::Start)))
            || !branches.iter().all(|b| match b.kind() {
                HirKind::Look(Look::Start) => true,
                HirKind::Class(Class::Unicode(c)) => !c
                    .iter()
                    .any(|r| (r.start()..=r.end()).contains(&crate::source_regex::MARKER)),
                _ => false,
            })
        {
            return None;
        }
        regex::Regex::new(&format!("(?:{left}){right}"))
            .ok()
            .map(Self::Delimited)
    }

    pub(super) fn could_match(&self, joined: &str) -> bool {
        match self {
            Self::Unknown => true,
            Self::Delimited(regex) => regex.is_match(joined),
            Self::Prefixes(prefixes) => prefixes.iter().any(|prefix| {
                joined
                    .as_bytes()
                    .windows(prefix.len())
                    .any(|window| window == prefix)
            }),
            Self::All(plans) => plans.iter().all(|plan| plan.could_match(joined)),
            Self::Any(plans) => plans.iter().any(|plan| plan.could_match(joined)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scoped_case_flags_and_optional_branches_remain_conservative() {
        for (pattern, input) in [
            (r"(?i:dob)(?-i:PIN)[0-9]+", "DObPIN123"),
            (r"tax|.*", "anything"),
            (r"(?i:tax)?[0-9]+", "1234"),
            (r"(?i)Steuernummer: [0-9]+", "StEuErNuMmEr: 1234567890"),
            (r"(?i)Steuernummer: [0-9]+", "ſTEUERNUMMER: 1234567890"),
        ] {
            assert!(Necessary::new(pattern).could_match(input), "{pattern}");
        }
        assert!(!Necessary::new(r"(?i:dob)(?-i:PIN)[0-9]+").could_match("dobpin123"));
        assert!(!Necessary::new(r"(?i)DOB: [0-9-]+").could_match("Card: 4111111111111111"));
        assert!(
            !Necessary::new(r"(?i)(?:dob|出生日期): [0-9-]+").could_match("Card: 4111111111111111")
        );
        assert!(
            Necessary::new(r"(?i)(?:dob|出生日期): [0-9-]+").could_match("出生日期: 2000-01-02")
        );
        assert!(!Necessary::marker_free("word\u{2060}".as_bytes()));
        assert!(!Necessary::marker_free(b"word\xe2\x81"));
        assert!(Necessary::marker_free("出生日期".as_bytes()));
    }
}
