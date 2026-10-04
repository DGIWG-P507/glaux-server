//! SWE extents are pairs, not subclasses of scalar values.
use crate::scalar::{NilDeclaration, ScalarComponent, ScalarValue};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RangeKind {
    Category,
    Count,
    Quantity,
    Time,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RangeEndpoint {
    pub value: ScalarValue,
    /// A reserved value's declared reason, never inferred from null or absence.
    pub nil_reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RangeComponent {
    pub kind: RangeKind,
    /// Shared endpoint metadata/constraints, with no scalar inline value.
    pub endpoint: ScalarComponent,
    pub nil_values: Vec<NilDeclaration>,
    pub value: Option<[RangeEndpoint; 2]>,
}
