#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(dead_code)]
#![allow(unused_assignments)]
#![allow(irrefutable_let_patterns)]
use std::{default, fmt};

use num_traits::{Bounded, Num, Zero};
use rust_intervals::Interval;
use rustc_middle::mir::{BinOp, UnOp};
// use std::ops::Range;
use std::ops::{Add, Mul, Sub};

use crate::{
    analysis::core::range_analysis::{
        Range, RangeType,
        domain::SymbolicExpr::{IntervalTypeTrait, SymbExpr},
    },
    rap_trace,
};

use super::domain::*;

// fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
//     let lower: &Lazy<String> = if self.range.left.0 == T::min_value() {
//         &STR_MIN
//     } else if self.range.left.0 == T::max_value() {
//         &STR_MAX
//     } else {
//         static DUMMY: Lazy<String> = Lazy::new(|| String::new());
//         let tmp = format!("{}", self.range.left.0);
//         let tmp_clone = tmp.clone();
//         let local = Lazy::new(|| tmp);
//         return write!(
//             f,
//             "{} [{}, {}]",
//             self.rtype,
//             *local,
//             if self.range.right.0 == T::min_value() {
//                 &*STR_MIN
//             } else if self.range.right.0 == T::max_value() {
//                 &*STR_MAX
//             } else {
//                 return write!(f, "{} [{}, {}]", self.rtype, tmp_clone, self.range.right.0);
//             }
//         );
//     };

//     let upper: &Lazy<String> = if self.range.right.0 == T::min_value() {
//         &STR_MIN
//     } else if self.range.right.0 == T::max_value() {
//         &STR_MAX
//     } else {
//         let tmp = format!("{}", self.range.right.0);
//         let local = Lazy::new(|| tmp);
//         return write!(f, "{} [{}, {}]", self.rtype, &**lower, *local);
//     };

//     write!(f, "{} [{}, {}]", self.rtype, &**lower, &**upper)
// }
impl<'tcx, T> Range<'tcx, T>
where
    T: IntervalArithmetic + Clone,
{
    // Parameterized constructor
    pub fn new(lb: T, ub: T, rtype: RangeType) -> Self {
        Self {
            rtype,
            lower: lb,
            upper: ub,
            lower_expr: SymbExpr::Unknown,
            upper_expr: SymbExpr::Unknown,
        }
    }

    pub fn new_symb(
        lb: T,
        ub: T,
        lower_expr: SymbExpr<'tcx, T>,
        upper_expr: SymbExpr<'tcx, T>,
        rtype: RangeType,
    ) -> Self {
        Self {
            rtype,
            lower: lb,
            upper: ub,
            lower_expr,
            upper_expr,
        }
    }

    pub fn default(default: T) -> Self {
        Self {
            rtype: RangeType::Unknown,
            lower: default,
            upper: default,
            lower_expr: SymbExpr::Unknown,
            upper_expr: SymbExpr::Unknown,
        }
    }

    pub fn init(lb: T, ub: T) -> Self {
        Self {
            rtype: RangeType::Regular,
            lower: lb,
            upper: ub,
            lower_expr: SymbExpr::Unknown,
            upper_expr: SymbExpr::Unknown,
        }
    }

    // Getter for lower bound
    pub fn get_lower(&self) -> T {
        self.lower.clone()
    }

    // Getter for upper bound
    pub fn get_upper(&self) -> T {
        self.upper.clone()
    }
    pub fn get_lower_expr(&self) -> SymbExpr<'tcx, T> {
        self.lower_expr.clone()
    }
    pub fn get_upper_expr(&self) -> SymbExpr<'tcx, T> {
        self.upper_expr.clone()
    }
    // Check if the range type is unknown
    pub fn is_unknown(&self) -> bool {
        self.rtype == RangeType::Unknown
    }

    // Set the range type to unknown
    pub fn set_unknown(&mut self) {
        self.rtype = RangeType::Unknown;
    }

    // Check if the range type is regular
    pub fn is_regular(&self) -> bool {
        self.rtype == RangeType::Regular
    }

    // Set the range type to regular
    pub fn set_regular(&mut self) {
        self.rtype = RangeType::Regular;
    }

    // Check if the range type is empty
    pub fn is_empty(&self) -> bool {
        self.rtype == RangeType::Empty
    }

    // Set the range type to empty
    pub fn set_empty(&mut self) {
        self.rtype = RangeType::Empty;
    }

    pub fn set_default(&mut self) {
        self.rtype = RangeType::Regular;
        self.lower = T::min_value();
        self.upper = T::max_value();
        self.lower_expr = SymbExpr::Unknown;
        self.upper_expr = SymbExpr::Unknown;
    }

    pub fn add(&self, other: &Range<'tcx, T>) -> Range<'tcx, T> {
        let a = self
            .get_lower()
            .checked_add(&other.get_lower())
            .unwrap_or(T::max_value());

        let b = self
            .get_upper()
            .checked_add(&other.get_upper())
            .unwrap_or(T::max_value());

        let a_expr = self.lower_expr.add(&other.lower_expr);
        let b_expr = self.upper_expr.add(&other.upper_expr);

        Range::new_symb(a, b, a_expr, b_expr, RangeType::Regular)
    }

    pub fn sub(&self, other: &Range<'tcx, T>) -> Range<'tcx, T> {
        let a = self
            .get_lower()
            .checked_sub(&other.get_upper())
            .unwrap_or(T::min_value());

        let b = self
            .get_upper()
            .checked_sub(&other.get_lower())
            .unwrap_or(T::max_value());

        let a_expr = self.lower_expr.sub(&other.upper_expr);
        let b_expr = self.upper_expr.sub(&other.lower_expr);

        Range::new_symb(a, b, a_expr, b_expr, RangeType::Regular)
    }

    pub fn mul(&self, other: &Range<'tcx, T>) -> Range<'tcx, T> {
        let candidates = vec![
            (
                self.get_lower() * other.get_lower(),
                self.lower_expr.mul(&other.lower_expr),
            ),
            (
                self.get_lower() * other.get_upper(),
                self.lower_expr.mul(&other.upper_expr),
            ),
            (
                self.get_upper() * other.get_lower(),
                self.upper_expr.mul(&other.lower_expr),
            ),
            (
                self.get_upper() * other.get_upper(),
                self.upper_expr.mul(&other.upper_expr),
            ),
        ];

        let (min_val, min_expr) = candidates
            .iter()
            .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap())
            .unwrap()
            .clone();

        let (max_val, max_expr) = candidates
            .iter()
            .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap())
            .unwrap()
            .clone();

        Range::new_symb(min_val, max_val, min_expr, max_expr, RangeType::Regular)
    }

    pub fn intersectwith(&self, other: &Range<'tcx, T>) -> Range<'tcx, T> {
        if self.is_unknown() {
            return other.clone();
        } else if other.is_unknown() {
            return self.clone();
        } else {
            let (left, left_expr) = match self.get_lower().partial_cmp(&other.get_lower()).unwrap()
            {
                std::cmp::Ordering::Greater | std::cmp::Ordering::Equal => {
                    (self.get_lower(), self.lower_expr.clone())
                }
                std::cmp::Ordering::Less => (other.get_lower(), other.lower_expr.clone()),
            };

            let (right, right_expr) =
                match self.get_upper().partial_cmp(&other.get_upper()).unwrap() {
                    std::cmp::Ordering::Less | std::cmp::Ordering::Equal => {
                        (self.get_upper(), self.upper_expr.clone())
                    }
                    std::cmp::Ordering::Greater => (other.get_upper(), other.upper_expr.clone()),
                };

            if left <= right {
                Range::new_symb(left, right, left_expr, right_expr, RangeType::Regular)
            } else {
                let mut empty_range = Range::default(T::min_value());
                empty_range.set_empty();
                empty_range
            }
        }
    }

    pub fn unionwith(&self, other: &Range<'tcx, T>) -> Range<'tcx, T> {
        if self.is_unknown() {
            return other.clone();
        } else if other.is_unknown() {
            return self.clone();
        } else {
            let (left, left_expr) = match self.get_lower().partial_cmp(&other.get_lower()).unwrap()
            {
                std::cmp::Ordering::Less | std::cmp::Ordering::Equal => {
                    (self.get_lower(), self.lower_expr.clone())
                }
                std::cmp::Ordering::Greater => (other.get_lower(), other.lower_expr.clone()),
            };

            let (right, right_expr) =
                match self.get_upper().partial_cmp(&other.get_upper()).unwrap() {
                    std::cmp::Ordering::Greater | std::cmp::Ordering::Equal => {
                        (self.get_upper(), self.upper_expr.clone())
                    }
                    std::cmp::Ordering::Less => (other.get_upper(), other.upper_expr.clone()),
                };

            Range::new_symb(left, right, left_expr, right_expr, RangeType::Regular)
        }
    }
}
pub struct Meet;

impl Meet {
    pub fn widen<'tcx, T: IntervalArithmetic + ConstConvert>(
        op: &mut BasicOpKind<'tcx, T>,
        constant_vector: &[T],
        vars: &mut VarNodes<'tcx, T>,
    ) -> bool {
        let old_interval = op.get_intersect().get_range().clone();
        let new_interval = op.eval(vars);

        let old_lower = old_interval.get_lower();
        let old_upper = old_interval.get_upper();
        let new_lower = new_interval.get_lower();
        let new_upper = new_interval.get_upper();
        let nlconstant = new_lower.clone();
        let nuconstant = new_upper.clone();

        let updated = if old_interval.is_unknown() {
            new_interval
        } else if new_lower < old_lower && new_upper > old_upper {
            Range::new(nlconstant, nuconstant, RangeType::Regular)
        } else if new_lower < old_lower {
            Range::new(nlconstant, old_upper.clone(), RangeType::Regular)
        } else if new_upper > old_upper {
            Range::new(old_lower.clone(), nuconstant, RangeType::Regular)
        } else {
            old_interval.clone()
        };

        let sink = op.get_sink().clone();

        op.set_intersect(updated.clone());

        vars.get_mut(&sink).unwrap().set_range(updated.clone());

        rap_trace!("WIDEN::{:?}: {:?} -> {:?}", sink, old_interval, updated);

        old_interval != updated
    }

    pub fn narrow<'tcx, T: IntervalArithmetic + ConstConvert>(
        op: &mut BasicOpKind<'tcx, T>,
        vars: &mut VarNodes<'tcx, T>,
    ) -> bool {
        let old_range = vars[op.get_sink()].get_range();
        let o_lower = old_range.get_lower().clone();
        let o_upper = old_range.get_upper().clone();

        let new_range = op.eval(vars);
        let n_lower = new_range.get_lower().clone();
        let n_upper = new_range.get_upper().clone();

        let mut has_changed = false;
        let min = T::min_value();
        let max = T::max_value();

        let mut result_lower = o_lower.clone();
        let mut result_upper = o_upper.clone();

        if o_lower == min && n_lower != min {
            result_lower = n_lower;
            has_changed = true;
        } else {
            let smin = T::min_value();
            if o_lower != smin {
                result_lower = smin;
                has_changed = true;
            }
        }

        if o_upper == max && n_upper != max {
            result_upper = n_upper;
            has_changed = true;
        } else {
            let smax = T::max_value();
            if o_upper != smax {
                result_upper = smax;
                has_changed = true;
            }
        }

        if has_changed {
            let new_sink_range = Range::new(
                result_lower.clone(),
                result_upper.clone(),
                RangeType::Regular,
            );
            let sink_node = vars.get_mut(op.get_sink()).unwrap();
            sink_node.set_range(new_sink_range.clone());
        }

        has_changed
    }
}
