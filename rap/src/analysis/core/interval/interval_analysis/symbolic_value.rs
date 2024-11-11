use rug::Integer;
use std::collections::HashSet;
use std::fmt::{Debug, Formatter, Result};
use std::hash::Hash;
use std::hash::Hasher;
use std::rc::Rc;

/// Represent a symbolic value. This is mainly used as our memory model
#[derive(Clone, Eq, Ord, PartialOrd)]
pub struct SymbolicValue {
    pub expression: Expression,
    pub expression_size: u64,
}

impl Debug for SymbolicValue {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        self.expression.fmt(f)
    }
}

impl Hash for SymbolicValue {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.expression.hash(state);
    }
}

impl PartialEq for SymbolicValue {
    fn eq(&self, other: &Self) -> bool {
        match (&self.expression, &other.expression) {
            // Assume widened values are equal
            (Expression::Widen { path: p1, .. }, Expression::Widen { path: p2, .. }) => p1.eq(p2),
            (e1, e2) => e1.eq(e2),
        }
    }
}

/// An abstract domain element that all represent the impossible concrete value.
/// I.e. the corresponding set of possible concrete values is empty.
pub const BOTTOM: SymbolicValue = SymbolicValue {
    expression: Expression::Bottom,
    expression_size: 1,
};

/// An abstract domain element that all represents all possible concrete values.
pub const TOP: SymbolicValue = SymbolicValue {
    expression: Expression::Top,
    expression_size: 1,
};