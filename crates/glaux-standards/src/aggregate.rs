//! Bounded, ordered DataRecord/Vector/DataChoice descriptions, not wire codecs.
//!
//! Inline children reuse scalar/range checks. Frame and semantic links remain
//! unresolved; no URI, coordinate transformation or component graph is fetched.
use std::collections::{BTreeMap, BTreeSet};

use glaux_domain::aggregate::{AggregateComponent, AggregateMetadata, Component, NamedComponent};
use glaux_domain::array::ArrayKind;
use serde_json::{Value, value::RawValue};

use crate::{
    array::{self, ArrayOptions, SourceValidation},
    choice::{CheckedComponentValue, ComponentError, ComponentValue},
    geometry::{GeometryContract, GeometryError},
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
    MissingElementCount,
    ElementCount,
    CountReference,
    MatrixElement,
    InlineElementValue,
    AdaptationSourceChanged,
    Scalar(ScalarError),
    Range(RangeError),
    Geometry(GeometryError),
}

enum ChildContract {
    Scalar(Box<ScalarContract>),
    Range(Box<RangeContract>),
    Aggregate(Box<AggregateContract>),
    Geometry(Box<GeometryContract>),
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

    pub fn geometry(&self) -> Option<&GeometryContract> {
        match &self.contract {
            ChildContract::Geometry(contract) => Some(contract),
            _ => None,
        }
    }

    pub fn source(&self) -> &[u8] {
        match &self.contract {
            ChildContract::Scalar(contract) => contract.source(),
            ChildContract::Range(contract) => contract.source(),
            ChildContract::Aggregate(contract) => contract.source(),
            ChildContract::Geometry(contract) => contract.source(),
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
            ChildContract::Geometry(contract) => {
                Component::Geometry(Box::new(contract.component().clone()))
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
    element_count_source: Option<Vec<u8>>,
    source_validation: SourceValidation,
}

impl AggregateContract {
    pub fn compile(validator: &StructuralValidator, input: &[u8]) -> Result<Self, AggregateError> {
        Self::compile_with_options(validator, input, ArrayOptions::default())
    }

    pub fn compile_with_options(
        validator: &StructuralValidator,
        input: &[u8],
        options: ArrayOptions,
    ) -> Result<Self, AggregateError> {
        // All byte/depth/node/member/string/numeric budgets and duplicate keys
        // are checked over the complete document before any tree traversal.
        let source = validation::parse(input).map_err(AggregateError::Syntax)?;
        preflight(&source).map_err(ComponentError::aggregate_kind)?;
        array::references(&source).map_err(ComponentError::aggregate_kind)?;
        // Preserve the established aggregate entry point's schema-first error
        // categories. The detailed choice entry point also locates child errors.
        let (contract, _) = kind(&source)?;
        array::structure(validator, contract, input, options)?;
        Self::compile_tree(validator, input, &source, options, None)
            .map_err(ComponentError::aggregate_kind)
    }

    pub(crate) fn compile_detailed_with_options(
        validator: &StructuralValidator,
        input: &[u8],
        options: ArrayOptions,
    ) -> Result<Self, ComponentError> {
        let source = validation::parse(input).map_err(AggregateError::Syntax)?;
        preflight(&source)?;
        array::references(&source)?;
        Self::compile_tree(validator, input, &source, options, None)
    }

    fn compile_tree(
        validator: &StructuralValidator,
        input: &[u8],
        source: &Value,
        options: ArrayOptions,
        inherited_frame: Option<&str>,
    ) -> Result<Self, ComponentError> {
        let (contract, member) = kind(source)?;
        let metadata = metadata(source)?;
        let (reference_frame, local_frame) =
            if matches!(contract, Contract::Vector | Contract::Matrix) {
                (
                    optional_string(source, "referenceFrame")?,
                    optional_string(source, "localFrame")?,
                )
            } else {
                (None, None)
            };
        let effective_frame = reference_frame.as_deref().or(inherited_frame);
        if contract == Contract::Vector && local_frame.is_some() && local_frame == reference_frame {
            return Err(AggregateError::Metadata.into());
        }

        // A Value round trip would lose JSON number spelling such as -0.
        let raw: BTreeMap<String, Box<RawValue>> =
            serde_json::from_slice(input).map_err(|_| AggregateError::Structure)?;
        let is_array = matches!(contract, Contract::DataArray | Contract::Matrix);
        let raw_children: Vec<Box<RawValue>> = if is_array {
            vec![
                RawValue::from_string(
                    raw.get(member)
                        .ok_or(AggregateError::Structure)?
                        .get()
                        .to_owned(),
                )
                .map_err(|_| AggregateError::Structure)?,
            ]
        } else {
            serde_json::from_str(raw.get(member).ok_or(AggregateError::Structure)?.get())
                .map_err(|_| AggregateError::Structure)?
        };
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
                    Some("Geometry") => ChildContract::Geometry(Box::new(
                        GeometryContract::compile(validator, bytes).map_err(AggregateError::Geometry)?,
                    )),
                    Some("Boolean" | "Text" | "Category" | "Count" | "Quantity" | "Time") => {
                        let scalar = if contract == Contract::Vector {
                            ScalarContract::compile_vector_coordinate(
                                validator,
                                bytes,
                                reference_frame.as_deref().ok_or(AggregateError::Metadata)?,
                            )
                        } else if contract == Contract::Matrix
                            && value.get("referenceFrame").is_none()
                            && let Some(frame) = effective_frame
                        {
                            ScalarContract::compile_vector_coordinate(validator, bytes, frame)
                        } else {
                            ScalarContract::compile(validator, bytes)
                        }
                        .map_err(AggregateError::Scalar)?;
                        ChildContract::Scalar(Box::new(scalar))
                    }
                    Some("CategoryRange" | "CountRange" | "QuantityRange" | "TimeRange") => {
                        // No implicit source correction or external category-order
                        // evidence is introduced by nesting an existing contract.
                        let range =
                            RangeContract::compile(validator, bytes, RangeOptions::default())
                                .map_err(AggregateError::Range)?;
                        ChildContract::Range(Box::new(range))
                    }
                    Some("DataRecord" | "Vector" | "DataChoice" | "DataArray" | "Matrix") => {
                        ChildContract::Aggregate(Box::new(Self::compile_tree(
                            validator,
                            bytes,
                            &value,
                            options,
                            if contract == Contract::Matrix {
                                effective_frame
                            } else {
                                None
                            },
                        )?))
                    }
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
        let source_validation = array::structure(validator, contract, input, options)?;
        let mut components: Vec<NamedComponent> =
            children.iter().map(NamedContract::component).collect();
        let element_count_source = raw
            .get("elementCount")
            .filter(|_| is_array)
            .map(|value| value.get().as_bytes().to_vec());
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
        } else if is_array {
            let element_count = array::count(
                source
                    .get("elementCount")
                    .ok_or(AggregateError::MissingElementCount)?,
                raw.get("elementCount")
                    .ok_or(AggregateError::MissingElementCount)?,
            )?;
            AggregateComponent::Array {
                kind: if contract == Contract::Matrix {
                    ArrayKind::Matrix
                } else {
                    ArrayKind::DataArray
                },
                metadata,
                element_count,
                element_type: Box::new(components.pop().ok_or(AggregateError::Structure)?),
                reference_frame,
                local_frame,
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
            element_count_source,
            source_validation,
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

    pub fn element_count_source(&self) -> Option<&[u8]> {
        self.element_count_source.as_deref()
    }

    pub fn source_validation(&self) -> SourceValidation {
        self.source_validation
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
        Some("DataArray") => Ok((Contract::DataArray, "elementType")),
        Some("Matrix") => Ok((Contract::Matrix, "elementType")),
        _ => Err(AggregateError::UnsupportedComponent),
    }
}

fn valid_name(name: &str) -> bool {
    // basicTypes.json NameToken: ^[A-Za-z][A-Za-z0-9_\-]*$
    let mut bytes = name.bytes();
    bytes
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

// Before the root schema check, identify out-of-scope descriptions/references
// explicitly rather than misreporting every valid-but-deferred type as invalid.
// Recursion is safe only because compile() first bounded the entire document.
fn preflight(source: &Value) -> Result<(), ComponentError> {
    let (contract, member) = match source.get("type").and_then(Value::as_str) {
        Some("DataRecord" | "Vector" | "DataChoice" | "DataArray" | "Matrix") => kind(source)?,
        Some(
            "Boolean" | "Text" | "Category" | "Count" | "Quantity" | "Time" | "CategoryRange"
            | "CountRange" | "QuantityRange" | "TimeRange" | "Geometry",
        ) => return Ok(()),
        _ => return Err(AggregateError::UnsupportedComponent.into()),
    };
    if ["value", "quality", "nilValues", "constraint"]
        .iter()
        .any(|member| source.get(member).is_some())
    {
        return Err(AggregateError::UnsupportedFeature.into());
    }
    if matches!(contract, Contract::DataArray | Contract::Matrix) {
        if ["encoding", "values"]
            .iter()
            .any(|member| source.get(member).is_some())
        {
            return Err(AggregateError::UnsupportedFeature.into());
        }
        if source.get("elementCount").is_none() {
            return Err(AggregateError::MissingElementCount.into());
        }
        let child = source.get("elementType").ok_or(AggregateError::Structure)?;
        if contract == Contract::Matrix
            && !matches!(
                child.get("type").and_then(Value::as_str),
                Some("Matrix" | "Count" | "Quantity" | "Time")
            )
        {
            return Err(ComponentError::from(AggregateError::MatrixElement).at(0));
        }
        no_element_values(child).map_err(|error| error.at(0))?;
        return preflight(child).map_err(|error| error.at(0));
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

pub(crate) fn optional_string(
    source: &Value,
    member: &str,
) -> Result<Option<String>, AggregateError> {
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

pub(crate) fn metadata(source: &Value) -> Result<AggregateMetadata, AggregateError> {
    Ok(AggregateMetadata {
        id: optional_string(source, "id")?,
        definition: optional_string(source, "definition")?,
        label: optional_string(source, "label")?,
        description: optional_string(source, "description")?,
        optional: source.get("optional").and_then(Value::as_bool),
        updatable: source.get("updatable").and_then(Value::as_bool),
    })
}

fn no_element_values(source: &Value) -> Result<(), ComponentError> {
    let kind = source.get("type").and_then(Value::as_str);
    if source.get("value").is_some()
        || (matches!(kind, Some("DataArray" | "Matrix")) && source.get("values").is_some())
    {
        return Err(AggregateError::InlineElementValue.into());
    }
    let member = match kind {
        Some("DataRecord") => Some("fields"),
        Some("Vector") => Some("coordinates"),
        Some("DataChoice") => Some("items"),
        _ => None,
    };
    if let Some(member) = member
        && let Some(children) = source.get(member).and_then(Value::as_array)
    {
        for (index, child) in children.iter().enumerate() {
            no_element_values(child).map_err(|error| error.at(index))?;
        }
    }
    if matches!(kind, Some("DataArray" | "Matrix"))
        && let Some(child) = source.get("elementType")
    {
        no_element_values(child).map_err(|error| error.at(0))?;
    }
    if kind == Some("DataChoice")
        && let Some(selector) = source.get("choiceValue")
    {
        no_element_values(selector)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
