//! Array shape declarations, never allocated array contents or resolved links.
use crate::{aggregate::AggregateMetadata, numeric::CountValue, scalar::NumericConstraint};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArrayKind {
    DataArray,
    Matrix,
}

/// The specialized ElementCount schema does not require scalar labels/types.
#[derive(Clone, Debug, PartialEq)]
pub struct CountDescriptor {
    pub metadata: AggregateMetadata,
    pub reference_frame: Option<String>,
    pub axis_id: Option<String>,
    pub constraint: Option<NumericConstraint>,
    /// None is a variable count, not an absent elementCount or a zero count.
    pub value: Option<CountValue>,
}

/// Preserved reference intent only; target and occurrence remain unresolved.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CountReference {
    pub href: String,
    pub role: Option<String>,
    pub arcrole: Option<String>,
    pub title: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ElementCount {
    Inline(Box<CountDescriptor>),
    Reference(CountReference),
}
