//! DataChoice selection at the shared-model boundary, not a wire decoder.
use glaux_domain::aggregate::AggregateComponent;

use crate::{
    aggregate::{AggregateContract, AggregateError, NamedContract},
    range::{CheckedRangeValue, RangeError},
    scalar::{CheckedScalarValue, ScalarContract, ScalarError},
    validation::{self, StructuralValidator},
};

mod value;

/// Already-decoded model input. No aggregate JSON/Text/Binary framing is implied.
/// Missing record members are distinct from supplied null or a nil sentinel.
pub enum ComponentValue<'a> {
    ScalarJson(&'a [u8]),
    RangeJson(&'a [u8]),
    Record(&'a [NamedValue<'a>]),
    Vector(&'a [NamedValue<'a>]),
    Choice(&'a [NamedValue<'a>]),
}

pub struct NamedValue<'a> {
    pub name: &'a str,
    pub value: ComponentValue<'a>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CheckedComponentValue {
    Scalar(Box<CheckedScalarValue>),
    Range(Box<CheckedRangeValue>),
    Record(Vec<CheckedNamedValue>),
    Vector {
        reference_frame: String,
        local_frame: Option<String>,
        coordinates: Vec<CheckedNamedValue>,
    },
    Choice(Box<CheckedChoiceValue>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct CheckedNamedValue {
    pub name: String,
    /// None means an omitted optional record member, never null or nil.
    pub value: Option<CheckedComponentValue>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CheckedChoiceValue {
    pub name: String,
    pub value: CheckedComponentValue,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComponentErrorKind {
    Compile(AggregateError),
    Scalar(ScalarError),
    Range(RangeError),
    ValueType,
    SelectionCardinality,
    UnknownSelection,
    UnknownMember,
    DuplicateMember,
    MissingMember,
    Limit,
}

/// Indices address declared children, not payload text. Unknown supplied names
/// identify the containing component; diagnostics never copy those names.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComponentError {
    pub kind: ComponentErrorKind,
    pub path: Vec<usize>,
}

impl ComponentError {
    pub(crate) fn new(kind: ComponentErrorKind) -> Self {
        Self {
            kind,
            path: Vec::new(),
        }
    }

    pub(crate) fn at(mut self, index: usize) -> Self {
        if self.path.len() < validation::MAX_DEPTH {
            self.path.insert(0, index);
        } else {
            self.kind = ComponentErrorKind::Limit;
        }
        self
    }

    pub(crate) fn aggregate_kind(self) -> AggregateError {
        match self.kind {
            ComponentErrorKind::Compile(error) => error,
            _ => AggregateError::Syntax(validation::Failure::Depth),
        }
    }
}

impl From<AggregateError> for ComponentError {
    fn from(error: AggregateError) -> Self {
        Self::new(ComponentErrorKind::Compile(error))
    }
}

/// Immutable alternatives; descriptor inline values never select a payload arm.
pub struct ChoiceContract {
    aggregate: AggregateContract,
}

impl ChoiceContract {
    pub fn compile(validator: &StructuralValidator, input: &[u8]) -> Result<Self, ComponentError> {
        let aggregate = AggregateContract::compile_detailed(validator, input)?;
        if !matches!(aggregate.component(), AggregateComponent::Choice { .. }) {
            return Err(AggregateError::UnsupportedComponent.into());
        }
        Ok(Self { aggregate })
    }

    pub fn component(&self) -> &AggregateComponent {
        self.aggregate.component()
    }

    pub fn source(&self) -> &[u8] {
        self.aggregate.source()
    }

    pub fn alternatives(&self) -> &[NamedContract] {
        self.aggregate.children()
    }

    pub fn choice_value(&self) -> Option<&ScalarContract> {
        self.aggregate.choice_value()
    }

    pub fn check_value(
        &self,
        selection: &[NamedValue<'_>],
    ) -> Result<CheckedChoiceValue, ComponentError> {
        match value::check(&self.aggregate, &ComponentValue::Choice(selection))? {
            CheckedComponentValue::Choice(value) => Ok(*value),
            _ => Err(ComponentError::new(ComponentErrorKind::ValueType)),
        }
    }
}

pub(crate) fn check_aggregate(
    contract: &AggregateContract,
    input: &ComponentValue<'_>,
) -> Result<CheckedComponentValue, ComponentError> {
    value::check(contract, input)
}

#[cfg(test)]
mod tests;
