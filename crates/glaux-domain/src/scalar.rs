//! SWE scalar meaning, independent of JSON, persistence and wire codecs.
//!
//! These are data types, not a validation capability. The standards package
//! constructs a checked, immutable contract from a bounded source description.

use crate::numeric::{CountValue, NumericValue};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComponentMetadata {
    pub id: Option<String>,
    pub definition: String,
    pub label: String,
    pub description: Option<String>,
    pub optional: Option<bool>,
    pub updatable: Option<bool>,
    pub reference_frame: Option<String>,
    pub axis_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TokenConstraint {
    Values(Vec<String>),
    Pattern(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum ScalarComponent {
    Boolean {
        metadata: ComponentMetadata,
        value: Option<bool>,
    },
    Text {
        metadata: ComponentMetadata,
        constraint: Option<TokenConstraint>,
        value: Option<String>,
    },
    Category {
        metadata: ComponentMetadata,
        code_space: Option<String>,
        constraint: Option<TokenConstraint>,
        value: Option<String>,
    },
    Count {
        metadata: ComponentMetadata,
        constraint: Option<NumericConstraint>,
        value: Option<CountValue>,
    },
    Quantity {
        metadata: ComponentMetadata,
        constraint: Option<NumericConstraint>,
        uom: UnitReference,
        value: Option<NumericValue>,
    },
}

/// Retain the complete supplied unit declaration, not a converted display unit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnitReference {
    pub label: Option<String>,
    pub symbol: Option<String>,
    pub code: Option<String>,
    pub href: Option<String>,
}

/// Enumeration and inclusive intervals form a union, not an intersection.
#[derive(Clone, Debug, PartialEq)]
pub struct NumericConstraint {
    pub values: Vec<NumericValue>,
    pub intervals: Vec<[NumericValue; 2]>,
    pub significant_figures: Option<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ScalarValue {
    Boolean(bool),
    Text(String),
    Category(String),
    Count(CountValue),
    Quantity(NumericValue),
}

impl ScalarComponent {
    /// Absence is not false, empty text, a nil token or a default value.
    pub fn value(&self) -> Option<ScalarValue> {
        match self {
            Self::Boolean { value, .. } => value.map(ScalarValue::Boolean),
            Self::Text { value, .. } => value.clone().map(ScalarValue::Text),
            Self::Category { value, .. } => value.clone().map(ScalarValue::Category),
            Self::Count { value, .. } => value.clone().map(ScalarValue::Count),
            Self::Quantity { value, .. } => value.clone().map(ScalarValue::Quantity),
        }
    }
}
