use std::{collections::HashMap, fmt};

use miette::{Diagnostic, LabeledSpan, SourceSpan};
use nom::{
    combinator::{all_consuming, complete, consumed},
    error::ParseError,
    Err, Finish, IResult, Parser,
};
use thiserror::Error;

#[derive(Debug, Clone, Eq, PartialEq, Error)]
#[error("{error}")]
pub struct LabeledError<K: LabeledErrorKind> {
    full_input: String,
    labels: Vec<LabeledSpan>,
    error: ErrorTree<K, Self>,
}

#[derive(Debug, Clone, Eq, PartialEq, Error)]
enum ErrorTree<K, E> {
    #[error("{kind}")]
    Node {
        kind: K,
        #[source]
        source: Option<Box<E>>,
    },
    #[error("attempted {} parse branches unsuccessfully", .0.len())]
    Branch(Vec<E>),
}

impl<E: LabeledErrorKind> Diagnostic for LabeledError<E> {
    fn source_code(&self) -> Option<&dyn miette::SourceCode> {
        Some(&self.full_input)
    }

    fn help<'a>(&'a self) -> Option<Box<dyn fmt::Display + 'a>> {
        if let ErrorTree::Node { kind, .. } = &self.error {
            kind.help()
        } else {
            None
        }
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = miette::LabeledSpan> + '_>> {
        Some(Box::new(self.labels.iter().cloned()))
    }

    fn related<'a>(&'a self) -> Option<Box<dyn Iterator<Item = &'a dyn Diagnostic> + 'a>> {
        if let ErrorTree::Branch(related) = &self.error {
            Some(Box::new(related.iter().map(|e| e as &dyn Diagnostic)))
        } else {
            None
        }
    }

    fn diagnostic_source(&self) -> Option<&dyn Diagnostic> {
        if let ErrorTree::Node { source, .. } = &self.error {
            source.as_ref().map(|e| &**e as &dyn Diagnostic)
        } else {
            None
        }
    }
}

impl<E: LabeledErrorKind> LabeledError<E> {
    fn bubble_labels(&mut self) {
        // FIXME: This eventually needs testing — showing that labels with different spans *don't* get merged
        fn merge_labels(labels: impl Iterator<Item = LabeledSpan>) -> Vec<LabeledSpan> {
            let mut span_map: HashMap<SourceSpan, Vec<String>> = HashMap::new();
            for labeled_span in labels {
                let span = labeled_span.inner();
                // FIXME: Gross with the clone() and to_owned() in here...
                let label = labeled_span.label().unwrap().to_owned();
                span_map
                    .entry(*span)
                    .and_modify(|l| l.push(label.clone()))
                    .or_insert_with(|| vec![label]);
            }
            span_map
                .into_iter()
                .map(|(span, labels)| {
                    let label = labels.join(" or ");
                    LabeledSpan::new_with_span(Some(label), span)
                })
                .collect()
        }

        if self.labels.is_empty() {
            match &mut self.error {
                ErrorTree::Node {
                    source: Some(child),
                    ..
                } => {
                    child.bubble_labels();
                    self.labels = child.labels.drain(..).collect();
                }
                ErrorTree::Branch(alternatives) => {
                    let new_labels = alternatives.iter_mut().flat_map(|child| {
                        child.bubble_labels();
                        child.labels.drain(..)
                    });
                    self.labels = merge_labels(new_labels);
                }
                ErrorTree::Node { .. } => (),
            }
        }
    }
}

// FIXME: Check that field ordering everywhere matches this!
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum LabeledParseError<'a, K> {
    Node {
        input: &'a str,
        length: usize,
        kind: K,
        source: Option<Box<Self>>,
    },
    Branch(Vec<Self>),
}

pub trait LabeledErrorKind: Diagnostic + Clone + From<nom::error::ErrorKind> {
    fn label(&self) -> Option<&'static str> {
        None
    }
}

pub trait FromExternalError<'a, E> {
    const FATAL: bool = false;
    fn from_external_error(input: &'a str, error: E) -> Self;
}

impl<'a, E: LabeledErrorKind> LabeledParseError<'a, E> {
    pub fn new(input: &'a str, kind: E) -> Self {
        Self::new_with_source(input, kind, None)
    }

    pub fn new_with_source(
        input: &'a str,
        kind: E,
        source: Option<LabeledParseError<'a, E>>,
    ) -> Self {
        Self::Node {
            input,
            length: 0,
            kind,
            source: source.map(Box::new),
        }
    }

    fn into_final_error(self, full_input: &str) -> LabeledError<E> {
        fn convert_node<E: LabeledErrorKind>(
            node: LabeledParseError<E>,
            full_input: &str,
        ) -> LabeledError<E> {
            // NOTE: The additional space is added so that Diagnostic labels can point to the end of an input
            let padded_input = format!("{full_input} ");

            match node {
                LabeledParseError::Node {
                    input,
                    length,
                    kind,
                    source,
                } => {
                    let source = source.map(|n| Box::new(convert_node(*n, full_input)));
                    let label = kind.label().map(str::to_string);
                    let span = span_from_input(full_input, input, length);
                    let labels = label
                        .into_iter()
                        .map(|l| LabeledSpan::new_with_span(Some(l), span))
                        .collect();
                    LabeledError {
                        full_input: padded_input,
                        labels,
                        error: { ErrorTree::Node { kind, source } },
                    }
                }
                LabeledParseError::Branch(alternatives) => {
                    let alternatives = alternatives
                        .into_iter()
                        .map(|n| convert_node(n, full_input))
                        .collect();
                    LabeledError {
                        full_input: padded_input,
                        labels: Vec::new(),
                        error: ErrorTree::Branch(alternatives),
                    }
                }
            }
        }

        let mut final_error = convert_node(self, full_input);
        final_error.bubble_labels();
        final_error
    }
}

fn span_from_input(full_input: &str, input: &str, length: usize) -> SourceSpan {
    let base_addr = full_input.as_ptr() as usize;
    let substr_addr = input.as_ptr() as usize;
    let start = substr_addr - base_addr;
    SourceSpan::new(start.into(), length)
}

pub fn final_parser<'a, O, P, E>(parser: P) -> impl FnMut(&'a str) -> Result<O, LabeledError<E>>
where
    E: LabeledErrorKind,
    P: Parser<&'a str, O, LabeledParseError<'a, E>>,
{
    let mut final_parser = all_consuming(complete(parser));
    move |input| {
        final_parser
            .parse(input)
            .finish()
            .map(|(_, c)| c)
            .map_err(|e| e.into_final_error(input))
    }
}

// FIXME: Why are these generics so much messier than map_res from nom?
pub fn map_res<'a, O1, O2, E1, E2, F, G>(
    parser: F,
    mut f: G,
) -> impl FnMut(&'a str) -> IResult<&'a str, O2, LabeledParseError<'a, E1>>
where
    O1: Clone,
    E1: LabeledErrorKind,
    F: Copy + Parser<&'a str, O1, LabeledParseError<'a, E1>>,
    G: Copy + FnMut(O1) -> Result<O2, E2>,
    LabeledParseError<'a, E1>: FromExternalError<'a, E2>,
{
    move |input| {
        let i = input;
        let (input, (consumed, o1)) = consumed(parser)(input)?;
        match f(o1) {
            Ok(o2) => Ok((input, o2)),
            Err(e) => {
                let mut e = LabeledParseError::from_external_error(i, e);
                if let LabeledParseError::Node { length, .. } = &mut e {
                    *length = consumed.len();
                }

                Err(if LabeledParseError::FATAL {
                    Err::Failure(e)
                } else {
                    Err::Error(e)
                })
            }
        }
    }
}

// FIXME: Check if I'm being consistent about using `impl` or generics...
// FIXME: See if this signature can be simplified (elide lifetimes?)
// FIXME: Standardize the order of all of these generic arguments!
pub fn wrap_err<'a, O, P, E>(
    mut parser: P,
    kind: E,
) -> impl FnMut(&'a str) -> IResult<&'a str, O, LabeledParseError<'a, E>>
where
    // FIXME: Eek, this is a where, and below (in expect) is not!
    E: LabeledErrorKind + Clone,
    P: Parser<&'a str, O, LabeledParseError<'a, E>>,
{
    // FIXME: DRY with expect below!
    move |i| {
        parser
            .parse(i)
            .map_err(|e| e.map(|e| LabeledParseError::new_with_source(i, kind.clone(), Some(e))))
    }
}

pub fn expect<'a, O, E: LabeledErrorKind + Clone, F>(
    mut parser: F,
    kind: E,
) -> impl FnMut(&'a str) -> IResult<&'a str, O, LabeledParseError<'a, E>>
where
    F: Parser<&'a str, O, LabeledParseError<'a, E>>,
{
    move |i| {
        parser
            .parse(i)
            .map_err(|e| e.map(|_| LabeledParseError::new(i, kind.clone())))
    }
}

// FIXME: Eventually, I should make everything generic over the input type again... So you'd be able to use this
// library with &'a [u8] like `nom` lets you
impl<'a, E: LabeledErrorKind> ParseError<&'a str> for LabeledParseError<'a, E> {
    fn from_error_kind(input: &'a str, kind: nom::error::ErrorKind) -> Self {
        Self::new(input, kind.into())
    }

    fn append(_input: &str, _kind: nom::error::ErrorKind, other: Self) -> Self {
        other
    }

    fn or(self, other: Self) -> Self {
        let alternatives = match self {
            Self::Node { .. } => Vec::new(),
            Self::Branch(ref a) => a.clone(),
        };
        Self::Branch([vec![self, other], alternatives].concat())
    }
}
