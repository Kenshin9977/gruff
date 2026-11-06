//! Typed views over the untyped tree, generated from `grammar/gsc.ungram`.
//!
//! Every view is a thin wrapper around a [`SyntaxNode`]; accessors return `None` when the parser
//! had to recover from an error, so they never panic.

mod ext;
mod generated;

use std::marker::PhantomData;

pub use generated::*;

use crate::{SyntaxKind, SyntaxNode, SyntaxNodeChildren, SyntaxToken};

/// A typed view of a node of one kind (or of a set of kinds, for enums).
pub trait AstNode {
    fn can_cast(kind: SyntaxKind) -> bool
    where
        Self: Sized;
    fn cast(syntax: SyntaxNode) -> Option<Self>
    where
        Self: Sized;
    fn syntax(&self) -> &SyntaxNode;
}

/// The children of a node that cast to `N`.
#[derive(Debug, Clone)]
pub struct AstChildren<N> {
    inner: SyntaxNodeChildren,
    ph: PhantomData<N>,
}

impl<N: AstNode> Iterator for AstChildren<N> {
    type Item = N;
    fn next(&mut self) -> Option<N> {
        self.inner.find_map(N::cast)
    }
}

pub(crate) mod support {
    use super::{AstChildren, AstNode};
    use crate::{SyntaxKind, SyntaxNode, SyntaxToken};

    pub(crate) fn child<N: AstNode>(parent: &SyntaxNode, nth: usize) -> Option<N> {
        parent.children().filter_map(N::cast).nth(nth)
    }

    pub(crate) fn children<N: AstNode>(parent: &SyntaxNode) -> AstChildren<N> {
        AstChildren {
            inner: parent.children(),
            ph: std::marker::PhantomData,
        }
    }

    pub(crate) fn token(parent: &SyntaxNode, kind: SyntaxKind) -> Option<SyntaxToken> {
        parent
            .children_with_tokens()
            .filter_map(|e| e.into_token())
            .find(|t| t.kind() == kind)
    }
}

/// The first non-trivia token directly under `node`.
pub(crate) fn first_token(node: &SyntaxNode) -> Option<SyntaxToken> {
    node.children_with_tokens()
        .filter_map(|e| e.into_token())
        .find(|t| !t.kind().is_trivia())
}

/// The first non-trivia token anywhere under `node` (descendants included, in source order).
/// Unlike the direct-children lookup the accessors use, this finds the first token of a nested
/// expression, which is where a diagnostic about the node should point.
pub fn first_significant_token(node: &SyntaxNode) -> Option<SyntaxToken> {
    node.descendants_with_tokens()
        .filter_map(|e| e.into_token())
        .find(|t| !t.kind().is_trivia())
}
