use gcollections::ops::*;
use interval::{ops::*, Interval};
use rustc_mir_dataflow::lattice::{HasBottom, HasTop, JoinSemiLattice, MeetSemiLattice};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntervalSet {
    Bottom,
    Elem(Interval<i32>),
    Top,
}
impl JoinSemiLattice for IntervalSet {
    fn join(&mut self, other: &Self) -> bool {
         match (&*self, other) {
            (Self::Top, _) | (_, Self::Top) => return false,
            (Self::Bottom, other) => {
                *self = other.clone();
                return true;
            }
            (_, Self::Bottom) => return false,
            (Self::Elem(interval_a), Self::Elem(interval_b)) => {
                *self = Self::Elem(interval_a.intersection(interval_b));
                return false;
            }
        };
    }
}
