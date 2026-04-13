#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(dead_code)]
#![allow(unused_assignments)]
#![allow(unused_parens)]
#![allow(non_snake_case)]
use rust_intervals::NothingBetween;

use crate::analysis::core::range_analysis::domain::ConstraintGraph::ConstraintGraph;
use crate::analysis::core::range_analysis::domain::domain::{
    ConstConvert, IntervalArithmetic, VarNode, VarNodes,
};
use crate::analysis::core::range_analysis::{Range, RangeType};
use crate::{rap_debug, rap_trace};
use num_traits::{Bounded, CheckedAdd, CheckedSub, One, ToPrimitive, Zero, ops};
use rustc_abi::FieldIdx;
use rustc_abi::Size;
use rustc_data_structures::fx::FxHashMap;
use rustc_hir::def_id::DefId;
use rustc_index::IndexVec;
use rustc_middle::mir::coverage::Op;
use rustc_middle::mir::{
    BasicBlock, BinOp, BorrowKind, CastKind, Const, Local, LocalDecl, Operand, Place, Rvalue,
    Statement, StatementKind, Terminator, UnOp,
};
use rustc_middle::ty::{ScalarInt, Ty};
use rustc_span::sym::no_default_passes;
use std::cell::RefCell;
use std::cmp::PartialEq;
use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::hash::Hash;
use std::ops::{Add, Mul, Sub};
use std::rc::Rc;
use std::{fmt, mem};
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BoundMode {
    Lower,
    Upper,
}

impl BoundMode {
    fn flip(self) -> Self {
        match self {
            BoundMode::Lower => BoundMode::Upper,
            BoundMode::Upper => BoundMode::Lower,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SymbExpr<'tcx> {
    Constant(Const<'tcx>),

    Place(&'tcx Place<'tcx>),

    Binary(BinOp, Box<SymbExpr<'tcx>>, Box<SymbExpr<'tcx>>),

    Unary(UnOp, Box<SymbExpr<'tcx>>),

    Cast(CastKind, Box<SymbExpr<'tcx>>, Ty<'tcx>),

    Unknown,
}
impl<'tcx> fmt::Display for SymbExpr<'tcx> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}
impl<'tcx> SymbExpr<'tcx> {
    fn const_bits(c: &Const<'tcx>) -> Option<u128> {
        c.try_to_scalar_int().map(|s| s.to_bits(s.size()))
    }

    fn is_const_zero(expr: &SymbExpr<'tcx>) -> bool {
        match expr {
            SymbExpr::Constant(c) => Self::const_bits(c) == Some(0),
            _ => false,
        }
    }

    fn is_const_one(expr: &SymbExpr<'tcx>) -> bool {
        match expr {
            SymbExpr::Constant(c) => Self::const_bits(c) == Some(1),
            _ => false,
        }
    }

    fn try_simplify_constants(&self) -> Option<Self> {
        match self {
            SymbExpr::Binary(op, lhs, rhs) => match op {
                BinOp::Add | BinOp::AddUnchecked | BinOp::AddWithOverflow => {
                    if Self::is_const_zero(rhs) {
                        Some((**lhs).clone())
                    } else if Self::is_const_zero(lhs) {
                        Some((**rhs).clone())
                    } else {
                        None
                    }
                }
                BinOp::Sub | BinOp::SubUnchecked | BinOp::SubWithOverflow => {
                    if Self::is_const_zero(rhs) {
                        Some((**lhs).clone())
                    } else {
                        None
                    }
                }
                BinOp::Mul | BinOp::MulUnchecked | BinOp::MulWithOverflow => {
                    if Self::is_const_zero(lhs) {
                        Some((**lhs).clone())
                    } else if Self::is_const_zero(rhs) {
                        Some((**rhs).clone())
                    } else if Self::is_const_one(lhs) {
                        Some((**rhs).clone())
                    } else if Self::is_const_one(rhs) {
                        Some((**lhs).clone())
                    } else {
                        None
                    }
                }
                BinOp::Div => {
                    if Self::is_const_one(rhs) {
                        Some((**lhs).clone())
                    } else {
                        None
                    }
                }
                BinOp::BitAnd => {
                    if Self::is_const_zero(lhs) {
                        Some((**lhs).clone())
                    } else if Self::is_const_zero(rhs) {
                        Some((**rhs).clone())
                    } else {
                        None
                    }
                }
                BinOp::BitOr | BinOp::BitXor => {
                    if Self::is_const_zero(rhs) {
                        Some((**lhs).clone())
                    } else if Self::is_const_zero(lhs) {
                        Some((**rhs).clone())
                    } else {
                        None
                    }
                }
                BinOp::Shl | BinOp::Shr => {
                    if Self::is_const_zero(rhs) {
                        Some((**lhs).clone())
                    } else {
                        None
                    }
                }
                _ => None,
            },
            SymbExpr::Unary(UnOp::Neg, inner) => {
                if let SymbExpr::Unary(UnOp::Neg, nested) = &**inner {
                    Some((**nested).clone())
                } else {
                    None
                }
            }
            SymbExpr::Unary(UnOp::Not, inner) => {
                if let SymbExpr::Unary(UnOp::Not, nested) = &**inner {
                    Some((**nested).clone())
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn from_place_with_ctx(place: &'tcx Place<'tcx>, place_ctx: &Vec<&'tcx Place<'tcx>>) -> Self {
        let found_base = place_ctx
            .iter()
            .find(|&&p| p.local == place.local && p.projection.is_empty());

        match found_base {
            Some(&base_place) => SymbExpr::Place(base_place),
            None => SymbExpr::Place(place),
        }
    }

    fn first_aggregate_operand_expr(
        operands: &'tcx IndexVec<FieldIdx, Operand<'tcx>>,
        place_ctx: &Vec<&'tcx Place<'tcx>>,
    ) -> Self {
        operands
            .iter()
            .find_map(|operand| match operand {
                Operand::Copy(place) | Operand::Move(place) => {
                    Some(Self::from_place_with_ctx(&place, place_ctx))
                }
                Operand::Constant(c) => Some(SymbExpr::Constant(c.const_)),
            })
            .unwrap_or(SymbExpr::Unknown)
    }

    pub fn add(&self, other: &Self) -> Self {
        if matches!(self, SymbExpr::Unknown) || matches!(other, SymbExpr::Unknown) {
            return SymbExpr::Unknown;
        }

        let mut expr =
            SymbExpr::Binary(BinOp::Add, Box::new(self.clone()), Box::new(other.clone()));

        expr.simplify();
        expr
    }

    pub fn sub(&self, other: &Self) -> Self {
        if matches!(self, SymbExpr::Unknown) || matches!(other, SymbExpr::Unknown) {
            return SymbExpr::Unknown;
        }

        let mut expr =
            SymbExpr::Binary(BinOp::Sub, Box::new(self.clone()), Box::new(other.clone()));

        expr.simplify();
        expr
    }

    pub fn mul(&self, other: &Self) -> Self {
        if matches!(self, SymbExpr::Unknown) || matches!(other, SymbExpr::Unknown) {
            return SymbExpr::Unknown;
        }

        let mut expr =
            SymbExpr::Binary(BinOp::Mul, Box::new(self.clone()), Box::new(other.clone()));

        expr.simplify();
        expr
    }

    pub fn from_operand(op: &'tcx Operand<'tcx>, place_ctx: &Vec<&'tcx Place<'tcx>>) -> Self {
        match op {
            Operand::Copy(place) | Operand::Move(place) => {
                let found_base = place_ctx
                    .iter()
                    .find(|&&p| p.local == place.local && p.projection.is_empty());

                match found_base {
                    Some(&base_place) => SymbExpr::Place(base_place),

                    None => SymbExpr::Place(place),
                }
            }
            Operand::Constant(c) => SymbExpr::Constant(c.const_),
        }
    }

    pub fn from_rvalue(rvalue: &'tcx Rvalue<'tcx>, place_ctx: Vec<&'tcx Place<'tcx>>) -> Self {
        match rvalue {
            Rvalue::Use(op) => Self::from_operand(op, &place_ctx),
            Rvalue::BinaryOp(bin_op, box (lhs, rhs)) => {
                let left = Self::from_operand(lhs, &place_ctx);
                let right = Self::from_operand(rhs, &place_ctx);

                if matches!(left, SymbExpr::Unknown) || matches!(right, SymbExpr::Unknown) {
                    return SymbExpr::Unknown;
                }

                SymbExpr::Binary(*bin_op, Box::new(left), Box::new(right))
            }
            Rvalue::UnaryOp(un_op, op) => {
                let expr = Self::from_operand(op, &place_ctx);
                if matches!(expr, SymbExpr::Unknown) {
                    return SymbExpr::Unknown;
                }
                SymbExpr::Unary(*un_op, Box::new(expr))
            }
            Rvalue::Cast(kind, op, ty) => {
                let expr = Self::from_operand(op, &place_ctx);
                if matches!(expr, SymbExpr::Unknown) {
                    return SymbExpr::Unknown;
                }
                SymbExpr::Cast(*kind, Box::new(expr), *ty)
            }
            Rvalue::Aggregate(_, operands) => {
                Self::first_aggregate_operand_expr(operands, &place_ctx)
            }

            Rvalue::Ref(..)
            | Rvalue::ThreadLocalRef(..)
            | Rvalue::Repeat(..)
            | Rvalue::ShallowInitBox(..)
            | Rvalue::NullaryOp(..)
            | Rvalue::Discriminant(..)
            | Rvalue::CopyForDeref(..) => SymbExpr::Unknown,
            Rvalue::RawPtr(raw_ptr_kind, place) => todo!(),
            Rvalue::WrapUnsafeBinder(operand, ty) => todo!(),
        }
    }

    pub fn from_rvalue_ssa(rvalue: &'tcx Rvalue<'tcx>, place_ctx: Vec<&'tcx Place<'tcx>>) -> Self {
        match rvalue {
            Rvalue::Aggregate(_, operands) => {
                Self::first_aggregate_operand_expr(operands, &place_ctx)
            }
            _ => Self::from_rvalue(rvalue, place_ctx),
        }
    }

    pub fn from_rvalue_essa(
        rvalue: &'tcx Rvalue<'tcx>,
        place_ctx: Vec<&'tcx Place<'tcx>>,
        sym_itv: Option<(&'tcx Place<'tcx>, BinOp)>,
    ) -> Self {
        match rvalue {
            Rvalue::Aggregate(_, operands) => {
                let lhs = Self::first_aggregate_operand_expr(operands, &place_ctx);
                if let Some((bound, predicate)) = sym_itv {
                    let rhs = Self::from_place_with_ctx(bound, &place_ctx);
                    SymbExpr::Binary(predicate, Box::new(lhs), Box::new(rhs))
                } else {
                    lhs
                }
            }
            _ => Self::from_rvalue(rvalue, place_ctx),
        }
    }

    // pub fn eval<T: IntervalArithmetic + ConstConvert + Debug>(
    //     &self,
    //     vars: &VarNodes<'tcx, T>,
    // ) -> Range<'tcx,T> {
    //     match self {
    //         SymbExpr::Unknown => Range::new(T::min_value(), T::max_value(), RangeType::Regular),

    //         SymbExpr::Constant(c) => {
    //             if let Some(val) = T::from_const(c) {
    //                 Range::new(val, val, RangeType::Regular)
    //             } else {
    //                 Range::new(T::min_value(), T::max_value(), RangeType::Regular)
    //             }
    //         }

    //         SymbExpr::Place(place) => {
    //             if let Some(node) = vars.get(place) {
    //                 node.get_range().clone()
    //             } else {
    //                 Range::new(T::min_value(), T::max_value(), RangeType::Regular)
    //             }
    //         }

    //         SymbExpr::Binary(op, lhs, rhs) => {
    //             let l_range = lhs.eval(vars);
    //             let r_range = rhs.eval(vars);

    //             match op {
    //                 BinOp::Add | BinOp::AddUnchecked | BinOp::AddWithOverflow => {
    //                     l_range.add(&r_range)
    //                 }
    //                 BinOp::Sub | BinOp::SubUnchecked | BinOp::SubWithOverflow => {
    //                     l_range.sub(&r_range)
    //                 }
    //                 BinOp::Mul | BinOp::MulUnchecked | BinOp::MulWithOverflow => {
    //                     l_range.mul(&r_range)
    //                 }

    //                 _ => Range::new(T::min_value(), T::max_value(), RangeType::Regular),
    //             }
    //         }

    //         SymbExpr::Unary(op, inner) => {
    //             let _inner_range = inner.eval(vars);
    //             match op {
    //                 UnOp::Neg => Range::new(T::min_value(), T::max_value(), RangeType::Regular),
    //                 UnOp::Not | UnOp::PtrMetadata => {
    //                     Range::new(T::min_value(), T::max_value(), RangeType::Regular)
    //                 }
    //             }
    //         }

    //         SymbExpr::Cast(kind, inner, _target_ty) => {
    //             let inner_range = inner.eval(vars);
    //             match kind {
    //                 CastKind::IntToInt => inner_range,

    //                 _ => Range::new(T::min_value(), T::max_value(), RangeType::Regular),
    //             }
    //         }
    //     }
    // }
    pub fn resolve_upper_bound<T: IntervalArithmetic + ConstConvert + Debug + Clone + PartialEq>(
        &mut self,
        vars: &VarNodes<'tcx, T>,
    ) {
        self.resolve_recursive(vars, 0, BoundMode::Upper);
    }
    pub fn resolve_lower_bound<T: IntervalArithmetic + ConstConvert + Debug + Clone + PartialEq>(
        &mut self,
        vars: &VarNodes<'tcx, T>,
    ) {
        self.resolve_recursive(vars, 0, BoundMode::Lower);
    }

    fn resolve_recursive<T: IntervalArithmetic + ConstConvert + Debug + Clone + PartialEq>(
        &mut self,
        vars: &VarNodes<'tcx, T>,
        depth: usize,
        mode: BoundMode,
    ) {
        const MAX_DEPTH: usize = 10;
        if depth > MAX_DEPTH {
            *self = SymbExpr::Unknown;
            return;
        }

        match self {
            SymbExpr::Binary(op, lhs, rhs) => {
                lhs.resolve_recursive(vars, depth + 1, mode);

                match op {
                    BinOp::Add | BinOp::AddUnchecked | BinOp::AddWithOverflow => {
                        rhs.resolve_recursive(vars, depth + 1, mode);
                    }
                    BinOp::Sub | BinOp::SubUnchecked | BinOp::SubWithOverflow => {
                        rhs.resolve_recursive(vars, depth + 1, mode.flip());
                    }
                    _ => rhs.resolve_recursive(vars, depth + 1, mode),
                }
            }
            SymbExpr::Unary(op, inner) => match op {
                UnOp::Neg => {
                    inner.resolve_recursive(vars, depth + 1, mode.flip());
                }
                _ => inner.resolve_recursive(vars, depth + 1, mode),
            },
            SymbExpr::Cast(_, inner, _) => {
                inner.resolve_recursive(vars, depth + 1, mode);
            }
            _ => {}
        }

        // self.try_fold_constants::<T>();
        rap_trace!("symexpr {}", self);
        if let SymbExpr::Place(place) = self {
            if let Some(node) = vars.get(place) {
                if let IntervalType::Basic(basic) = &node.interval {
                    rap_trace!("node {:?}", *node);

                    let target_expr =
                        if basic.range.get_lower_expr() == basic.range.get_upper_expr() {
                            &basic.range.get_upper_expr()
                        } else {
                            match mode {
                                BoundMode::Upper => &basic.range.get_upper_expr(),
                                BoundMode::Lower => &basic.range.get_lower_expr(),
                            }
                        };

                    match target_expr {
                        SymbExpr::Unknown => *self = SymbExpr::Unknown,
                        SymbExpr::Constant(c) => *self = SymbExpr::Constant(c.clone()),
                        expr => {
                            if let SymbExpr::Place(target_place) = expr {
                                if target_place == place {
                                    return;
                                }
                            }

                            *self = expr.clone();
                            self.resolve_recursive(vars, depth + 1, mode);
                        }
                    }
                }
            }
        }
    }
    pub fn simplify(&mut self) {
        match self {
            SymbExpr::Binary(_, lhs, rhs) => {
                lhs.simplify();
                rhs.simplify();
            }
            SymbExpr::Unary(_, inner) => {
                inner.simplify();
            }
            SymbExpr::Cast(_, inner, _) => {
                inner.simplify();
            }
            _ => {}
        }

        if let Some(simplified) = self.try_simplify_constants() {
            *self = simplified;
            return;
        }

        if let Some(simplified) = self.try_flatten_linear() {
            *self = simplified;
            return;
        }
    }

    fn try_flatten_linear(&self) -> Option<Self> {
        if !matches!(
            self,
            SymbExpr::Binary(
                BinOp::Add
                    | BinOp::AddUnchecked
                    | BinOp::AddWithOverflow
                    | BinOp::Sub
                    | BinOp::SubUnchecked
                    | BinOp::SubWithOverflow,
                _,
                _
            ) | SymbExpr::Unary(UnOp::Neg, _)
        ) {
            return None;
        }

        let mut terms = Vec::new();
        self.extract_linear_terms(1, &mut terms);

        // 合并同类项 (遍历寻找 PartialEq 相等的项，累加系数)
        let mut merged: Vec<(i128, SymbExpr<'tcx>)> = Vec::new();
        for (sign, term) in terms {
            if let Some(existing) = merged.iter_mut().find(|(_, t)| t == &term) {
                existing.0 += sign;
            } else {
                merged.push((sign, term));
            }
        }

        merged.retain(|(coeff, _)| *coeff != 0);

        if merged.is_empty() {
            return Some(SymbExpr::Unknown);
        }

        let mut pos_terms = vec![];
        let mut neg_terms = vec![];

        for (coeff, term) in merged {
            if coeff > 0 {
                for _ in 0..coeff {
                    pos_terms.push(term.clone());
                }
            } else {
                for _ in 0..(-coeff) {
                    neg_terms.push(term.clone());
                }
            }
        }

        let pos_tree = Self::build_sum_tree(pos_terms);
        let neg_tree = Self::build_sum_tree(neg_terms);

        match (pos_tree, neg_tree) {
            (Some(p), Some(n)) => Some(SymbExpr::Binary(BinOp::Sub, Box::new(p), Box::new(n))),
            (Some(p), None) => Some(p),
            (None, Some(n)) => Some(SymbExpr::Unary(UnOp::Neg, Box::new(n))),
            (None, None) => unreachable!(),
        }
    }

    fn extract_linear_terms(&self, sign: i128, terms: &mut Vec<(i128, SymbExpr<'tcx>)>) {
        match self {
            SymbExpr::Binary(op, box lhs, box rhs) => match op {
                BinOp::Add | BinOp::AddUnchecked | BinOp::AddWithOverflow => {
                    lhs.extract_linear_terms(sign, terms);
                    rhs.extract_linear_terms(sign, terms);
                }
                BinOp::Sub | BinOp::SubUnchecked | BinOp::SubWithOverflow => {
                    lhs.extract_linear_terms(sign, terms);
                    rhs.extract_linear_terms(-sign, terms);
                }
                _ => terms.push((sign, self.clone())),
            },
            SymbExpr::Unary(UnOp::Neg, box inner) => {
                inner.extract_linear_terms(-sign, terms);
            }
            _ => terms.push((sign, self.clone())),
        }
    }

    fn build_sum_tree(mut terms: Vec<SymbExpr<'tcx>>) -> Option<SymbExpr<'tcx>> {
        if terms.is_empty() {
            return None;
        }
        let mut tree = terms.remove(0);
        for term in terms {
            tree = SymbExpr::Binary(BinOp::Add, Box::new(tree), Box::new(term));
        }
        Some(tree)
    }
}
#[derive(Debug, Clone)]
pub enum IntervalType<'tcx, T: IntervalArithmetic + ConstConvert + Debug> {
    Basic(BasicInterval<'tcx, T>),
    Symb(SymbInterval<'tcx, T>),
}

impl<'tcx, T: IntervalArithmetic + ConstConvert + Debug> fmt::Display for IntervalType<'tcx, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IntervalType::Basic(b) => write!(f, "BasicInterval: {:?}  ", b.get_range(),),
            IntervalType::Symb(b) => write!(f, "SymbInterval: {:?}  ", b.get_range(),),
        }
    }
}
pub trait IntervalTypeTrait<'tcx, T: IntervalArithmetic + ConstConvert + Debug> {
    fn get_range(&self) -> &Range<'tcx, T>;
    fn set_range(&mut self, new_range: Range<'tcx, T>);
    fn get_lower_expr(&self) -> &SymbExpr<'tcx>;
    fn get_upper_expr(&self) -> &SymbExpr<'tcx>;
}
impl<'tcx, T: IntervalArithmetic + ConstConvert + Debug> IntervalTypeTrait<'tcx, T>
    for IntervalType<'tcx, T>
{
    fn get_range(&self) -> &Range<'tcx, T> {
        match self {
            IntervalType::Basic(b) => b.get_range(),
            IntervalType::Symb(s) => s.get_range(),
        }
    }

    fn set_range(&mut self, new_range: Range<'tcx, T>) {
        match self {
            IntervalType::Basic(b) => b.set_range(new_range),
            IntervalType::Symb(s) => s.set_range(new_range),
        }
    }
    fn get_lower_expr(&self) -> &SymbExpr<'tcx> {
        match self {
            IntervalType::Basic(b) => b.get_lower_expr(),
            IntervalType::Symb(s) => s.get_lower_expr(),
        }
    }

    fn get_upper_expr(&self) -> &SymbExpr<'tcx> {
        match self {
            IntervalType::Basic(b) => b.get_upper_expr(),
            IntervalType::Symb(s) => s.get_upper_expr(),
        }
    }
}
#[derive(Debug, Clone)]

pub struct BasicInterval<'tcx, T: IntervalArithmetic + ConstConvert + Debug> {
    pub range: Range<'tcx, T>,
}

impl<'tcx, T: IntervalArithmetic + ConstConvert + Debug> BasicInterval<'tcx, T> {
    pub fn new(range: Range<'tcx, T>) -> Self {
        Self { range }
    }
    pub fn new_symb(lower: SymbExpr<'tcx>, upper: SymbExpr<'tcx>) -> Self {
        Self {
            range: Range {
                rtype: RangeType::Regular,
                lower: T::min_value(),
                upper: T::max_value(),
                lower_expr: lower,
                upper_expr: upper,
            },
        }
    }
    pub fn default() -> Self {
        Self {
            range: Range::default(T::min_value()),
        }
    }
}
impl<'tcx, T: IntervalArithmetic + ConstConvert + Debug> IntervalTypeTrait<'tcx, T>
    for BasicInterval<'tcx, T>
{
    // fn get_value_id(&self) -> IntervalId {
    //     IntervalId::BasicIntervalId
    // }

    fn get_range(&self) -> &Range<'tcx, T> {
        &self.range
    }

    fn set_range(&mut self, new_range: Range<'tcx, T>) {
        self.range = new_range;
        if self.range.get_lower() > self.range.get_upper() {
            self.range.set_empty();
        }
    }
    fn get_lower_expr(&self) -> &SymbExpr<'tcx> {
        &self.range.lower_expr
    }

    fn get_upper_expr(&self) -> &SymbExpr<'tcx> {
        &self.range.upper_expr
    }
}

#[derive(Debug, Clone)]

pub struct SymbInterval<'tcx, T: IntervalArithmetic + ConstConvert + Debug> {
    range: Range<'tcx, T>,
    symbound: &'tcx Place<'tcx>,
    predicate: BinOp,
}

impl<'tcx, T: IntervalArithmetic + ConstConvert + Debug> SymbInterval<'tcx, T> {
    pub fn new(range: Range<'tcx, T>, symbound: &'tcx Place<'tcx>, predicate: BinOp) -> Self {
        Self {
            range,
            symbound,
            predicate,
        }
    }

    // pub fn refine(&mut self, vars: &VarNodes<'tcx, T>) {
    //     if let SymbExpr::Unknown = self.lower {
    //     } else {
    //         let low_range = self.lower.eval(vars);
    //         if low_range.get_lower() > self.range.get_lower() {
    //             let new_range = Range::new(
    //                 low_range.get_lower(),
    //                 self.range.get_upper(),
    //                 RangeType::Regular,
    //             );
    //             self.range = new_range;
    //         }
    //     }

    //     if let SymbExpr::Unknown = self.upper {
    //         // Do nothing
    //     } else {
    //         let high_range = self.upper.eval(vars);
    //         if high_range.get_upper() < self.range.get_upper() {
    //             let new_range = Range::new(
    //                 self.range.get_lower(),
    //                 high_range.get_upper(),
    //                 RangeType::Regular,
    //             );
    //             self.range = new_range;
    //         }
    //     }
    // }

    pub fn get_operation(&self) -> BinOp {
        self.predicate
    }

    pub fn get_bound(&self) -> &'tcx Place<'tcx> {
        self.symbound
    }

    pub fn sym_fix_intersects(
        &self,
        bound: &VarNode<'tcx, T>,
        sink: &VarNode<'tcx, T>,
    ) -> Range<'tcx, T> {
        let l = bound.get_range().get_lower().clone();
        let u = bound.get_range().get_upper().clone();

        let lower = sink.get_range().get_lower().clone();
        let upper = sink.get_range().get_upper().clone();

        match self.predicate {
            BinOp::Eq => Range::new(l, u, RangeType::Regular),

            BinOp::Le => Range::new(lower, u, RangeType::Regular),

            BinOp::Lt => {
                if u != T::max_value() {
                    let u_minus_1 = u.checked_sub(&T::one()).unwrap_or(u);
                    Range::new(lower, u_minus_1, RangeType::Regular)
                } else {
                    Range::new(lower, u, RangeType::Regular)
                }
            }

            BinOp::Ge => Range::new(l, upper, RangeType::Regular),

            BinOp::Gt => {
                if l != T::min_value() {
                    let l_plus_1 = l.checked_add(&T::one()).unwrap_or(l);
                    Range::new(l_plus_1, upper, RangeType::Regular)
                } else {
                    Range::new(l, upper, RangeType::Regular)
                }
            }

            BinOp::Ne => Range::new(T::min_value(), T::max_value(), RangeType::Regular),

            _ => Range::new(T::min_value(), T::max_value(), RangeType::Regular),
        }
    }
}

impl<'tcx, T: IntervalArithmetic + ConstConvert + Debug> IntervalTypeTrait<'tcx, T>
    for SymbInterval<'tcx, T>
{
    // fn get_value_id(&self) -> IntervalId {
    //     IntervalId::SymbIntervalId
    // }

    fn get_range(&self) -> &Range<'tcx, T> {
        &self.range
    }

    fn set_range(&mut self, new_range: Range<'tcx, T>) {
        self.range = new_range;
    }
    fn get_lower_expr(&self) -> &SymbExpr<'tcx> {
        &self.range.lower_expr
    }

    fn get_upper_expr(&self) -> &SymbExpr<'tcx> {
        &self.range.upper_expr
    }
}
