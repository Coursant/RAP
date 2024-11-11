use crate::analysis::memory::path::Path;
use crate::analysis::numerical::interval::{Bound, Interval};
use crate::analysis::numerical::lattice::LatticeTrait;
use crate::analysis::numerical::linear_constraint::{
    LinearConstraint, LinearConstraintSystem, LinearExpression,
};
pub mod option;
pub mod abstract_domain;
pub mod apron_domain;
pub mod lattice;
use crate::analysis::option::AbstractDomainType;
use apron_sys;
use foreign_types::foreign_type;
use foreign_types::{ForeignType, ForeignTypeRef, Opaque};
use rug::{Assign, Integer, Rational};
use std::collections::BTreeMap;
use std::convert::From;
use std::fmt::{self, Debug};
use std::marker::PhantomData;
use std::ptr::NonNull;
use std::rc::Rc;