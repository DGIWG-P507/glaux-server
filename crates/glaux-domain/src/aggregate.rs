//! Ordered SWE descriptions, independent of payload codecs or frame resolution.
use crate::{range::RangeComponent, scalar::ScalarComponent};

/// DataRecord does not require definition/label; retain absence, not empty text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AggregateMetadata {
    pub id: Option<String>,
    pub definition: Option<String>,
    pub label: Option<String>,
    pub description: Option<String>,
    pub optional: Option<bool>,
    pub updatable: Option<bool>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Component {
    Scalar(Box<ScalarComponent>),
    Range(Box<RangeComponent>),
    Aggregate(Box<AggregateComponent>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct NamedComponent {
    pub name: String,
    pub component: Component,
}

#[derive(Clone, Debug, PartialEq)]
pub enum AggregateComponent {
    Record {
        metadata: AggregateMetadata,
        fields: Vec<NamedComponent>,
    },
    Vector {
        metadata: AggregateMetadata,
        /// A supplied URI reference, not a resolved frame or transformation.
        reference_frame: String,
        local_frame: Option<String>,
        coordinates: Vec<NamedComponent>,
    },
}
