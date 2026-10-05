//! Bounded, ordered DataRecord/Vector/DataChoice descriptions, not wire codecs.
//!
//! Inline children reuse scalar/range checks. Frame and semantic links remain
//! unresolved; no URI, coordinate transformation or component graph is fetched.
use std::collections::{BTreeMap, BTreeSet};

use glaux_domain::aggregate::{AggregateComponent, AggregateMetadata, Component, NamedComponent};
use serde_json::{Value, value::RawValue};

use crate::{
    choice::{CheckedComponentValue, ComponentError, ComponentValue},
    range::{RangeContract, RangeError, RangeOptions},
    scalar::{ScalarContract, ScalarError},
    validation::{self, Contract, StructuralValidator},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AggregateError {
    Syntax(validation::Failure),
    Structure,
    UnsupportedComponent,
    UnsupportedFeature,
    Metadata,
    DuplicateName,
    EmptyVector,
    CoordinateType,
    CoordinateReferenceFrame,
    CoordinateAxis,
    OptionalCoordinate,
    ChoiceCardinality,
    Scalar(ScalarError),
    Range(RangeError),
}

enum ChildContract {
    Scalar(Box<ScalarContract>),
    Range(Box<RangeContract>),
    Aggregate(Box<AggregateContract>),
}

/// Each child retains its name and original JSON object, in declaration order.
pub struct NamedContract {
    name: String,
    contract: ChildContract,
}

impl NamedContract {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn scalar(&self) -> Option<&ScalarContract> {
        match &self.contract {
            ChildContract::Scalar(contract) => Some(contract),
            _ => None,
        }
    }

    pub fn range(&self) -> Option<&RangeContract> {
        match &self.contract {
            ChildContract::Range(contract) => Some(contract),
            _ => None,
        }
    }

    pub fn aggregate(&self) -> Option<&AggregateContract> {
        match &self.contract {
            ChildContract::Aggregate(contract) => Some(contract),
            _ => None,
        }
    }

    pub fn source(&self) -> &[u8] {
        match &self.contract {
            ChildContract::Scalar(contract) => contract.source(),
            ChildContract::Range(contract) => contract.source(),
            ChildContract::Aggregate(contract) => contract.source(),
        }
    }

    fn component(&self) -> NamedComponent {
        let component = match &self.contract {
            ChildContract::Scalar(contract) => {
                Component::Scalar(Box::new(contract.component().clone()))
            }
            ChildContract::Range(contract) => {
                Component::Range(Box::new(contract.component().clone()))
            }
            ChildContract::Aggregate(contract) => {
                Component::Aggregate(Box::new(contract.component().clone()))
            }
        };
        NamedComponent {
            name: self.name.clone(),
            component,
        }
    }
}

/// Immutable aggregate meaning and its independently checked child contracts.
pub struct AggregateContract {
    component: AggregateComponent,
    source: Vec<u8>,
    children: Vec<NamedContract>,
    choice_value: Option<ScalarContract>,
}

impl AggregateContract {
    pub fn compile(validator: &StructuralValidator, input: &[u8]) -> Result<Self, AggregateError> {
        // All byte/depth/node/member/string/numeric budgets and duplicate keys
        // are checked over the complete document before any tree traversal.
        let source = validation::parse(input).map_err(AggregateError::Syntax)?;
        preflight(&source).map_err(ComponentError::aggregate_kind)?;
        // Preserve the established aggregate entry point's schema-first error
        // categories. The detailed choice entry point also locates child errors.
        let (contract, _) = kind(&source)?;
        validator
            .validate(contract, input)
            .map_err(|_| AggregateError::Structure)?;
        Self::compile_tree(validator, input, &source).map_err(ComponentError::aggregate_kind)
    }

    pub(crate) fn compile_detailed(
        validator: &StructuralValidator,
        input: &[u8],
    ) -> Result<Self, ComponentError> {
        let source = validation::parse(input).map_err(AggregateError::Syntax)?;
        preflight(&source)?;
        Self::compile_tree(validator, input, &source)
    }

    fn compile_tree(
        validator: &StructuralValidator,
        input: &[u8],
        source: &Value,
    ) -> Result<Self, ComponentError> {
        let (contract, member) = kind(source)?;
        let metadata = metadata(source)?;
        let (reference_frame, local_frame) = if contract == Contract::Vector {
            (
                optional_string(source, "referenceFrame")?,
                optional_string(source, "localFrame")?,
            )
        } else {
            (None, None)
        };
        if contract == Contract::Vector && local_frame.is_some() && local_frame == reference_frame {
            return Err(AggregateError::Metadata.into());
        }

        // A Value round trip would lose JSON number spelling such as -0.
        let raw: BTreeMap<String, Box<RawValue>> =
            serde_json::from_slice(input).map_err(|_| AggregateError::Structure)?;
        let raw_children: Vec<Box<RawValue>> =
            serde_json::from_str(raw.get(member).ok_or(AggregateError::Structure)?.get())
                .map_err(|_| AggregateError::Structure)?;
        let mut children = Vec::with_capacity(raw_children.len());
        let mut names = BTreeSet::new();
        for child in raw_children {
            let index = children.len();
            let compiled = (|| -> Result<NamedContract, ComponentError> {
                let bytes = child.get().as_bytes();
                let value = validation::parse(bytes).map_err(AggregateError::Syntax)?;
                let name = optional_string(&value, "name")?.ok_or(AggregateError::Structure)?;
                // The wrapper owns NameToken; scalar child schemas do not.
                if !valid_name(&name) {
                    return Err(AggregateError::Structure.into());
                }
                if !names.insert(name.clone()) {
                    return Err(AggregateError::DuplicateName.into());
                }
                let child_contract = match value.get("type").and_then(Value::as_str) {
                    Some("Boolean" | "Text" | "Category" | "Count" | "Quantity" | "Time") => {
                        let scalar = if contract == Contract::Vector {
                            ScalarContract::compile_vector_coordinate(
                                validator,
                                bytes,
                                reference_frame.as_deref().ok_or(AggregateError::Metadata)?,
                            )
                        } else {
                            ScalarContract::compile(validator, bytes)
                        }
                        .map_err(AggregateError::Scalar)?;
                        ChildContract::Scalar(Box::new(scalar))
                    }
                    Some("CategoryRange" | "CountRange" | "QuantityRange" | "TimeRange") => {
                        // No implicit source correction or external category-order
                        // evidence is introduced by nesting an existing contract.
                        let range = RangeContract::compile(validator, bytes, RangeOptions::default())
                            .map_err(AggregateError::Range)?;
                        ChildContract::Range(Box::new(range))
                    }
                    Some("DataRecord" | "Vector" | "DataChoice") => ChildContract::Aggregate(Box::new(
                        Self::compile_tree(validator, bytes, &value)?,
                    )),
                    _ => return Err(AggregateError::UnsupportedComponent.into()),
                };
                Ok(NamedContract {
                    name,
                    contract: child_contract,
                })
            })()
            .map_err(|error| error.at(index))?;
            children.push(compiled);
        }
        let choice_value = if contract == Contract::DataChoice {
            raw.get("choiceValue")
                .map(|value| ScalarContract::compile(validator, value.get().as_bytes()))
                .transpose()
                .map_err(AggregateError::Scalar)?
        } else {
            None
        };
        // Locate child failures first, but never omit the original enclosing
        // schema check or replace it with independently passing child schemas.
        validator
            .validate(contract, input)
            .map_err(|_| AggregateError::Structure)?;
        let components = children.iter().map(NamedContract::component).collect();
        let component = if contract == Contract::Vector {
            AggregateComponent::Vector {
                metadata,
                reference_frame: reference_frame.ok_or(AggregateError::Metadata)?,
                local_frame,
                coordinates: components,
            }
        } else if contract == Contract::DataChoice {
            AggregateComponent::Choice {
                metadata,
                items: components,
                choice_value: choice_value
                    .as_ref()
                    .map(|value| Box::new(value.component().clone())),
            }
        } else {
            AggregateComponent::Record {
                metadata,
                fields: components,
            }
        };
        Ok(Self {
            component,
            source: input.to_vec(),
            children,
            choice_value,
        })
    }

    pub fn component(&self) -> &AggregateComponent {
        &self.component
    }

    pub fn source(&self) -> &[u8] {
        &self.source
    }

    pub fn children(&self) -> &[NamedContract] {
        &self.children
    }

    pub fn choice_value(&self) -> Option<&ScalarContract> {
        self.choice_value.as_ref()
    }

    pub fn check_value(
        &self,
        input: &ComponentValue<'_>,
    ) -> Result<CheckedComponentValue, ComponentError> {
        crate::choice::check_aggregate(self, input)
    }
}

fn kind(source: &Value) -> Result<(Contract, &'static str), AggregateError> {
    match source.get("type").and_then(Value::as_str) {
        Some("DataRecord") => Ok((Contract::DataRecord, "fields")),
        Some("Vector") => Ok((Contract::Vector, "coordinates")),
        Some("DataChoice") => Ok((Contract::DataChoice, "items")),
        _ => Err(AggregateError::UnsupportedComponent),
    }
}

fn valid_name(name: &str) -> bool {
    // basicTypes.json NameToken: ^[A-Za-z][A-Za-z0-9_\-]*$
    let mut bytes = name.bytes();
    bytes.next().is_some_and(|first| first.is_ascii_alphabetic())
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

// Before the root schema check, identify out-of-scope descriptions/references
// explicitly rather than misreporting every valid-but-deferred type as invalid.
// Recursion is safe only because compile() first bounded the entire document.
fn preflight(source: &Value) -> Result<(), ComponentError> {
    let (contract, member) = match source.get("type").and_then(Value::as_str) {
        Some("DataRecord" | "Vector" | "DataChoice") => kind(source)?,
        Some(
            "Boolean" | "Text" | "Category" | "Count" | "Quantity" | "Time" | "CategoryRange"
            | "CountRange" | "QuantityRange" | "TimeRange",
        ) => return Ok(()),
        _ => return Err(AggregateError::UnsupportedComponent.into()),
    };
    if ["value", "quality", "nilValues", "constraint"]
        .iter()
        .any(|member| source.get(member).is_some())
    {
        return Err(AggregateError::UnsupportedFeature.into());
    }
    let children = source
        .get(member)
        .and_then(Value::as_array)
        .ok_or(AggregateError::Structure)?;
    // SWE Vector UML is [1..*]; the original JSON omits minItems.
    if contract == Contract::Vector && children.is_empty() {
        return Err(AggregateError::EmptyVector.into());
    }
    // DataChoice UML item multiplicity is [2..*]; original JSON has no minItems.
    if contract == Contract::DataChoice && children.len() < 2 {
        return Err(AggregateError::ChoiceCardinality.into());
    }
    for (index, child) in children.iter().enumerate() {
        (|| -> Result<(), ComponentError> {
            if child.get("type").is_none() && child.get("href").is_some() {
                return Err(AggregateError::UnsupportedComponent.into());
            }
            if contract == Contract::Vector {
                if !matches!(
                    child.get("type").and_then(Value::as_str),
                    Some("Count" | "Quantity" | "Time")
                ) {
                    return Err(AggregateError::CoordinateType.into());
                }
                // Requirements 39/40: omit referenceFrame, require axisID. Even a
                // redundant, equal referenceFrame is a forbidden child declaration.
                if child.get("referenceFrame").is_some() {
                    return Err(AggregateError::CoordinateReferenceFrame.into());
                }
                if child
                    .get("axisID")
                    .and_then(Value::as_str)
                    .is_none_or(str::is_empty)
                {
                    return Err(AggregateError::CoordinateAxis.into());
                }
                if child.get("optional").and_then(Value::as_bool) == Some(true) {
                    return Err(AggregateError::OptionalCoordinate.into());
                }
            }
            preflight(child)
        })()
        .map_err(|error| error.at(index))?;
    }
    Ok(())
}

fn optional_string(source: &Value, member: &str) -> Result<Option<String>, AggregateError> {
    source
        .get(member)
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or(AggregateError::Metadata)
        })
        .transpose()
}

fn metadata(source: &Value) -> Result<AggregateMetadata, AggregateError> {
    Ok(AggregateMetadata {
        id: optional_string(source, "id")?,
        definition: optional_string(source, "definition")?,
        label: optional_string(source, "label")?,
        description: optional_string(source, "description")?,
        optional: source.get("optional").and_then(Value::as_bool),
        updatable: source.get("updatable").and_then(Value::as_bool),
    })
}

#[cfg(test)]
mod tests;
