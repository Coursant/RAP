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
use rustc_abi::Size;
use rustc_data_structures::fx::FxHashMap;
use rustc_hir::def_id::DefId;
use rustc_middle::mir::coverage::Op;
use rustc_middle::mir::{
    BasicBlock, BinOp, Body, BorrowKind, CastKind, Const, Local, LocalDecl, Operand, Place, Rvalue,
    Statement, StatementKind, Terminator, UnOp,
};
use rustc_middle::ty::{ParamEnv, ScalarInt, Ty, TyCtxt};
use rustc_span::sym::no_default_passes;
use std::cell::RefCell;
use std::cmp::PartialEq;
use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::hash::Hash;
use std::ops::{Add, Mul, Sub};
use std::rc::Rc;
use std::{fmt, mem};

use z3::ast::{Ast, BV, Int};
use z3::{Config, Context, Optimize, SatResult};

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
#[derive(Clone)]

pub struct Z3ExprBuilder<'ctx, 'tcx> {
    /// Global Z3 Context reference.
    pub z3_ctx: &'ctx z3::Context,
    pub tcx: TyCtxt<'tcx>,

    /// Rustc context and current function body for type information.
    pub body: &'tcx Body<'tcx>,

    /// Current function's DefId to prefix Z3 variables and prevent cross-function collisions.
    pub def_id: DefId,

    /// The preset bit-width for all BitVectors (e.g., 32 or 64).
    pub default_bitwidth: u32,

    /// Symbol table: Ensures the same Place always maps to the identical Z3 variable instance.
    pub place_cache: HashMap<Place<'tcx>, BV<'ctx>>,
}

impl<'ctx, 'tcx> Z3ExprBuilder<'ctx, 'tcx> {
    /// Initializes a new builder with a fixed bit-width for all expressions.
    pub fn new(
        tcx: TyCtxt<'tcx>,
        z3_ctx: &'ctx z3::Context,
        body: &'tcx Body<'tcx>,
        def_id: DefId,
        default_bitwidth: u32,
    ) -> Self {
        Self {
            tcx,
            z3_ctx,
            body,
            def_id,
            default_bitwidth,
            place_cache: HashMap::new(),
        }
    }

    /// Gets an existing Z3 variable for a Place, or creates a new one if it doesn't exist.
    pub fn get_or_create_place(&mut self, place: Place<'tcx>) -> BV<'ctx> {
        if let Some(var) = self.place_cache.get(&place) {
            return var.clone();
        }
        let func_name = self.tcx.def_path_str(self.def_id);
        // Add the function name as a namespace to prevent naming collisions in the global Context.
        let name = format!("{}::{:?}", func_name, place);

        let var = BV::new_const(self.z3_ctx, name, self.default_bitwidth);

        self.place_cache.insert(place, var.clone());
        var
    }

    /// Parses a Z3 BV expression from an Operand.
    pub fn from_operand(
        &mut self,
        op: &'tcx Operand<'tcx>,
        place_ctx: &[&'tcx Place<'tcx>],
    ) -> Option<BV<'ctx>> {
        match op {
            Operand::Copy(place) | Operand::Move(place) => {
                // Find the base place if it exists in the context
                let found_base = place_ctx
                    .iter()
                    .find(|&&p| p.local == place.local && p.projection.is_empty());

                let target_place = match found_base {
                    Some(&base_place) => base_place,
                    None => place,
                };

                Some(self.get_or_create_place(*target_place))
            }
            Operand::Constant(c) => {
                // Or using your original approach depending on the rustc version:
                // let val = c.const_.try_to_scalar_int().unwrap().to_i128();
                let val = c.const_.try_to_scalar_int().unwrap().to_i64();
                // Construct a BitVector constant
                // BV::from_i64 handles the two's complement conversion internally
                Some(BV::from_i64(self.z3_ctx, val as i64, self.default_bitwidth))
            }
        }
    }

    /// Parses a Z3 BV expression directly from an Rvalue.
    pub fn from_rvalue(
        &mut self,
        rvalue: &'tcx Rvalue<'tcx>,
        place_ctx: &[&'tcx Place<'tcx>],
    ) -> Option<BV<'ctx>> {
        match rvalue {
            Rvalue::Use(op) => self.from_operand(op, place_ctx),

            Rvalue::BinaryOp(bin_op, box (lhs, rhs)) => {
                let left = self.from_operand(lhs, place_ctx)?;
                let right = self.from_operand(rhs, place_ctx)?;

                // Ensure operands have the same bit-width before applying binary ops
                if left.get_size() != right.get_size() {
                    return None; // Z3 requires matching sizes for BV operations
                }

                // Identify if the operation should be signed or unsigned based on operand type
                let ty = lhs.ty(self.body, self.tcx);
                let is_signed = ty.is_signed();

                match bin_op {
                    // Addition, Subtraction, and Multiplication are the same
                    // at the bit level for signed and unsigned integers (Two's complement)
                    BinOp::Add | BinOp::AddUnchecked | BinOp::AddWithOverflow => {
                        Some(left.bvadd(&right))
                    }
                    BinOp::Sub | BinOp::SubUnchecked | BinOp::SubWithOverflow => {
                        Some(left.bvsub(&right))
                    }
                    BinOp::Mul | BinOp::MulUnchecked | BinOp::MulWithOverflow => {
                        Some(left.bvmul(&right))
                    }

                    // Division and Remainder differ significantly between signed/unsigned
                    BinOp::Div if is_signed => Some(left.bvsdiv(&right)),
                    BinOp::Div => Some(left.bvudiv(&right)),
                    BinOp::Rem if is_signed => Some(left.bvsrem(&right)),
                    BinOp::Rem => Some(left.bvurem(&right)),

                    // Bitwise operations
                    BinOp::BitAnd => Some(left.bvand(&right)),
                    BinOp::BitOr => Some(left.bvor(&right)),
                    BinOp::BitXor => Some(left.bvxor(&right)),

                    // Shifts
                    BinOp::Shl | BinOp::ShlUnchecked => Some(left.bvshl(&right)),
                    BinOp::Shr | BinOp::ShrUnchecked if is_signed => Some(left.bvashr(&right)), // Arithmetic shift (preserves sign)
                    BinOp::Shr | BinOp::ShrUnchecked => Some(left.bvlshr(&right)), // Logical shift

                    _ => None,
                }
            }

            Rvalue::UnaryOp(un_op, op) => {
                let expr = self.from_operand(op, place_ctx)?;
                match un_op {
                    UnOp::Neg => Some(expr.bvneg()), // Two's complement negation
                    UnOp::Not => Some(expr.bvnot()), // Bitwise NOT
                    _ => None,
                }
            }

            Rvalue::Cast(kind, op, ty) => {
                let expr = self.from_operand(op, place_ctx)?;
                let target_width = self.get_ty_bit_width(*ty);
                let current_width = expr.get_size();

                match kind {
                    CastKind::IntToInt => {
                        if target_width > current_width {
                            // Extension: Sign-extend if the original type is signed, else zero-extend
                            let is_signed = op.ty(self.body, self.tcx).is_signed();
                            let extend_by = target_width - current_width;

                            if is_signed {
                                Some(expr.sign_ext(extend_by))
                            } else {
                                Some(expr.zero_ext(extend_by))
                            }
                        } else if target_width < current_width {
                            // Truncation: keep the lower `target_width` bits
                            Some(expr.extract(target_width - 1, 0))
                        } else {
                            // Same size, no modification needed in the BV domain
                            Some(expr)
                        }
                    }
                    // For floating-point or ptr casts, we skip modelling in this simplified domain
                    _ => None,
                }
            }

            // Ignore unhandled Rvalue variants
            _ => None,
        }
    }

    /// Helper to get the bit width of a rustc Ty.
    fn get_ty_bit_width(&self, ty: Ty<'tcx>) -> u32 {
        use rustc_middle::ty::TyKind;
        match ty.kind() {
            TyKind::Int(i) => i.bit_width().unwrap_or(self.default_bitwidth as u64) as u32,
            TyKind::Uint(u) => u.bit_width().unwrap_or(self.default_bitwidth as u64) as u32,
            TyKind::Bool => 1,
            // Use the target architecture's pointer size for pointer types
            TyKind::RawPtr(_, _) | TyKind::Ref(_, _, _) => {
                self.tcx.data_layout.pointer_size().bits() as u32
            }
            _ => self.default_bitwidth,
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
impl<'ctx, 'tcx> SymbExpr<'tcx> {
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
            Rvalue::Ref(..)
            | Rvalue::ThreadLocalRef(..)
            | Rvalue::Aggregate(..)
            | Rvalue::Repeat(..)
            | Rvalue::ShallowInitBox(..)
            | Rvalue::NullaryOp(..)
            | Rvalue::Discriminant(..)
            | Rvalue::CopyForDeref(..) => SymbExpr::Unknown,
            Rvalue::RawPtr(raw_ptr_kind, place) => todo!(),
            Rvalue::WrapUnsafeBinder(operand, ty) => todo!(),
        }
    }

    pub fn resolve_upper_bound<T: IntervalArithmetic + ConstConvert + Debug + Clone + PartialEq>(
        &mut self,
        vars: &VarNodes<'ctx, 'tcx, T>,
    ) {
        self.resolve_recursive(vars, 0, BoundMode::Upper);
    }
    pub fn resolve_lower_bound<T: IntervalArithmetic + ConstConvert + Debug + Clone + PartialEq>(
        &mut self,
        vars: &VarNodes<'ctx, 'tcx, T>,
    ) {
        self.resolve_recursive(vars, 0, BoundMode::Lower);
    }

    fn resolve_recursive<T: IntervalArithmetic + ConstConvert + Debug + Clone + PartialEq>(
        &mut self,
        vars: &VarNodes<'ctx, 'tcx, T>,
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

                    let target_expr = if basic.lower == basic.upper {
                        &basic.upper
                    } else {
                        match mode {
                            BoundMode::Upper => &basic.upper,
                            BoundMode::Lower => &basic.lower,
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

        if let SymbExpr::Binary(op, lhs, rhs) = self {
            match op {
                BinOp::Sub | BinOp::SubUnchecked | BinOp::SubWithOverflow => {
                    if let SymbExpr::Binary(inner_op, inner_lhs, inner_rhs) = lhs.as_ref() {
                        match inner_op {
                            BinOp::Add | BinOp::AddUnchecked | BinOp::AddWithOverflow => {
                                if inner_lhs == rhs {
                                    *self = *inner_rhs.clone();
                                } else if inner_rhs == rhs {
                                    *self = *inner_lhs.clone();
                                }
                            }
                            _ => {}
                        }
                    }
                }
                BinOp::Add | BinOp::AddUnchecked | BinOp::AddWithOverflow => {
                    if let SymbExpr::Binary(inner_op, inner_lhs, inner_rhs) = lhs.as_ref() {
                        match inner_op {
                            BinOp::Sub | BinOp::SubUnchecked | BinOp::SubWithOverflow => {
                                if inner_rhs == rhs {
                                    *self = *inner_lhs.clone();
                                }
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
    }
}
#[derive(Debug, Clone)]
pub enum IntervalType<'ctx, 'tcx, T: IntervalArithmetic + ConstConvert + Debug> {
    Basic(BasicInterval<'ctx, 'tcx, T>),
    Symb(SymbInterval<'ctx, 'tcx, T>),
}

impl<'ctx, 'tcx, T: IntervalArithmetic + ConstConvert + Debug> fmt::Display
    for IntervalType<'ctx, 'tcx, T>
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IntervalType::Basic(b) => write!(
                f,
                "BasicInterval: {:?} {:?} {:?} ",
                b.get_range(),
                b.lower,
                b.upper
            ),
            IntervalType::Symb(b) => write!(
                f,
                "SymbInterval: {:?} {:?} {:?} ",
                b.get_range(),
                b.lower,
                b.upper
            ),
        }
    }
}
pub trait IntervalTypeTrait<'ctx, 'tcx, T: IntervalArithmetic + ConstConvert + Debug> {
    fn get_range(&self) -> &Range<T>;
    fn set_range(&mut self, new_range: Range<T>);
    fn get_lower_expr(&self) -> &SymbExpr<'tcx>;
    fn get_upper_expr(&self) -> &SymbExpr<'tcx>;
}
impl<'ctx, 'tcx, T: IntervalArithmetic + ConstConvert + Debug> IntervalTypeTrait<'ctx, 'tcx, T>
    for IntervalType<'ctx, 'tcx, T>
{
    fn get_range(&self) -> &Range<T> {
        match self {
            IntervalType::Basic(b) => b.get_range(),
            IntervalType::Symb(s) => s.get_range(),
        }
    }

    fn set_range(&mut self, new_range: Range<T>) {
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

pub struct BasicInterval<'ctx, 'tcx, T: IntervalArithmetic + ConstConvert + Debug> {
    pub range: Range<T>,
    pub lower: SymbExpr<'tcx>,
    pub upper: SymbExpr<'tcx>,
    pub z3_lower: Option<BV<'ctx>>,
    pub z3_upper: Option<BV<'ctx>>,
}

impl<'ctx, 'tcx, T: IntervalArithmetic + ConstConvert + Debug> BasicInterval<'ctx, 'tcx, T> {
    pub fn new(range: Range<T>) -> Self {
        Self {
            range,
            lower: SymbExpr::Unknown,
            upper: SymbExpr::Unknown,
            z3_lower: None,
            z3_upper: None,
        }
    }
    pub fn new_symb(range: Range<T>, lower: SymbExpr<'tcx>, upper: SymbExpr<'tcx>) -> Self {
        Self {
            range,
            lower,
            upper,
            z3_lower: None,
            z3_upper: None,
        }
    }
    pub fn default() -> Self {
        Self {
            range: Range::default(T::min_value()),
            lower: SymbExpr::Unknown,
            upper: SymbExpr::Unknown,
            z3_lower: None,
            z3_upper: None,
        }
    }
}

impl<'ctx, 'tcx, T: IntervalArithmetic + ConstConvert + Debug> IntervalTypeTrait<'ctx, 'tcx, T>
    for BasicInterval<'ctx, 'tcx, T>
{
    // fn get_value_id(&self) -> IntervalId {
    //     IntervalId::BasicIntervalId
    // }

    fn get_range(&self) -> &Range<T> {
        &self.range
    }

    fn set_range(&mut self, new_range: Range<T>) {
        self.range = new_range;
        if self.range.get_lower() > self.range.get_upper() {
            self.range.set_empty();
        }
    }
    fn get_lower_expr(&self) -> &SymbExpr<'tcx> {
        &self.lower
    }

    fn get_upper_expr(&self) -> &SymbExpr<'tcx> {
        &self.upper
    }
}

#[derive(Debug, Clone)]

pub struct SymbInterval<'ctx, 'tcx, T: IntervalArithmetic + ConstConvert + Debug> {
    range: Range<T>,
    symbound: &'tcx Place<'tcx>,
    predicate: BinOp,
    lower: SymbExpr<'tcx>,
    upper: SymbExpr<'tcx>,
    z3_lower: Option<BV<'ctx>>,
    z3_upper: Option<BV<'ctx>>,
}

impl<'ctx, 'tcx, T: IntervalArithmetic + ConstConvert + Debug> SymbInterval<'ctx, 'tcx, T> {
    pub fn new(range: Range<T>, symbound: &'tcx Place<'tcx>, predicate: BinOp) -> Self {
        Self {
            range,
            symbound,
            predicate,
            lower: SymbExpr::Unknown,
            upper: SymbExpr::Unknown,
            z3_lower: None,
            z3_upper: None,
        }
    }

    pub fn get_operation(&self) -> BinOp {
        self.predicate
    }

    pub fn get_bound(&self) -> &'tcx Place<'tcx> {
        self.symbound
    }

    pub fn sym_fix_intersects(
        &self,
        bound: &VarNode<'ctx, 'tcx, T>,
        sink: &VarNode<'ctx, 'tcx, T>,
    ) -> Range<T> {
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

impl<'ctx, 'tcx, T: IntervalArithmetic + ConstConvert + Debug> IntervalTypeTrait<'ctx, 'tcx, T>
    for SymbInterval<'ctx, 'tcx, T>
{
    // fn get_value_id(&self) -> IntervalId {
    //     IntervalId::SymbIntervalId
    // }

    fn get_range(&self) -> &Range<T> {
        &self.range
    }

    fn set_range(&mut self, new_range: Range<T>) {
        self.range = new_range;
    }
    fn get_lower_expr(&self) -> &SymbExpr<'tcx> {
        &self.lower
    }

    fn get_upper_expr(&self) -> &SymbExpr<'tcx> {
        &self.upper
    }
}
