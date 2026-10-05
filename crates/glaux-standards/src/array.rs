//! DataArray/Matrix descriptions, without payload allocation or reference resolution.
use std::{collections::BTreeMap, sync::OnceLock};

use glaux_domain::{
    aggregate::AggregateComponent,
    array::{CountDescriptor, CountReference, ElementCount},
    numeric::CountValue,
};
use serde_json::{Value, json, value::RawValue};

use crate::{
    aggregate::{self, AggregateContract, AggregateError, NamedContract},
    choice::ComponentError,
    scalar,
    validation::{self, Contract, StructuralValidator},
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CountReferenceSchema {
    #[default]
    Original,
    /// Exclude href-bearing objects from the inline ElementCount branch.
    DisjointReferenceCorrection,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ArrayOptions {
    pub count_reference_schema: CountReferenceSchema,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceValidation {
    Original,
    /// Original wrapper failed; only the explicitly selected correction passed.
    CountReferenceCorrection,
}

pub struct ArrayContract {
    aggregate: AggregateContract,
}

impl ArrayContract {
    pub fn compile(
        validator: &StructuralValidator,
        input: &[u8],
        options: ArrayOptions,
    ) -> Result<Self, ComponentError> {
        let aggregate =
            AggregateContract::compile_detailed_with_options(validator, input, options)?;
        if !matches!(aggregate.component(), AggregateComponent::Array { .. }) {
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

    pub fn element(&self) -> &NamedContract {
        &self.aggregate.children()[0]
    }

    pub fn element_count_source(&self) -> &[u8] {
        self.aggregate.element_count_source().unwrap_or_default()
    }

    pub fn source_validation(&self) -> SourceValidation {
        self.aggregate.source_validation()
    }

    /// Directly nested dimensions in outer-to-inner order. A record/choice is
    /// an element, not another dimension; declared contents are never expanded.
    pub fn dimensions(&self) -> Vec<&ElementCount> {
        let mut dimensions = Vec::new();
        let mut current = Some(&self.aggregate);
        while let Some(contract) = current {
            let AggregateComponent::Array { element_count, .. } = contract.component() else {
                break;
            };
            dimensions.push(element_count);
            current = contract
                .children()
                .first()
                .and_then(NamedContract::aggregate);
        }
        dimensions
    }
}

pub(crate) fn count(source: &Value, raw: &RawValue) -> Result<ElementCount, AggregateError> {
    if source.get("href").is_some() {
        if [
            "type",
            "value",
            "constraint",
            "nilValues",
            "referenceFrame",
            "axisID",
        ]
        .iter()
        .any(|member| source.get(member).is_some())
        {
            return Err(AggregateError::CountReference);
        }
        let href =
            aggregate::optional_string(source, "href")?.ok_or(AggregateError::CountReference)?;
        scalar::check_format(&href, "uri-reference").map_err(|_| AggregateError::CountReference)?;
        if href.is_empty() || href == "#" {
            return Err(AggregateError::CountReference);
        }
        return Ok(ElementCount::Reference(CountReference {
            href,
            role: aggregate::optional_string(source, "role")?,
            arcrole: aggregate::optional_string(source, "arcrole")?,
            title: aggregate::optional_string(source, "title")?,
        }));
    }
    if source
        .get("type")
        .is_some_and(|kind| kind.as_str() != Some("Count"))
    {
        return Err(AggregateError::ElementCount);
    }
    if ["nilValues", "quality"]
        .iter()
        .any(|member| source.get(member).is_some())
    {
        return Err(AggregateError::UnsupportedFeature);
    }
    let raw: BTreeMap<String, Box<RawValue>> =
        serde_json::from_str(raw.get()).map_err(|_| AggregateError::ElementCount)?;
    let (constraint, value) =
        scalar::element_count(source, raw.get("value").map(|value| value.get()))
            .map_err(AggregateError::Scalar)?;
    let zero: CountValue = "0".parse().map_err(|_| AggregateError::ElementCount)?;
    // Fixed dimensions are positive. Zero belongs to variable payload sizes,
    // whose later decoder is not implemented by this description compiler.
    if value.as_ref().is_some_and(|value| value <= &zero) {
        return Err(AggregateError::ElementCount);
    }
    Ok(ElementCount::Inline(Box::new(CountDescriptor {
        metadata: aggregate::metadata(source)?,
        reference_frame: aggregate::optional_string(source, "referenceFrame")?,
        axis_id: aggregate::optional_string(source, "axisID")?,
        constraint,
        value,
    })))
}

pub(crate) fn structure(
    validator: &StructuralValidator,
    contract: Contract,
    input: &[u8],
    options: ArrayOptions,
) -> Result<SourceValidation, AggregateError> {
    if validator.validate(contract, input).is_ok() {
        return Ok(SourceValidation::Original);
    }
    if options.count_reference_schema != CountReferenceSchema::DisjointReferenceCorrection {
        return Err(AggregateError::Structure);
    }
    let source = validation::parse(input).map_err(AggregateError::Syntax)?;
    let corrected = corrected_validators().as_ref().map_err(|error| *error)?;
    let corrected = corrected
        .get(&contract)
        .ok_or(AggregateError::AdaptationSourceChanged)?;
    if corrected.is_valid(&source) {
        Ok(SourceValidation::CountReferenceCorrection)
    } else {
        Err(AggregateError::Structure)
    }
}

fn corrected_validators()
-> &'static Result<BTreeMap<Contract, jsonschema::Validator>, AggregateError> {
    static CORRECTED: OnceLock<Result<BTreeMap<Contract, jsonschema::Validator>, AggregateError>> =
        OnceLock::new();
    CORRECTED.get_or_init(|| {
        let mut catalog =
            validation::catalog().map_err(|_| AggregateError::AdaptationSourceChanged)?;
        let schema = catalog
            .get_mut(&Contract::DataArray.uri())
            .ok_or(AggregateError::AdaptationSourceChanged)?;
        correct_count_schema(schema)?;
        crate::schema_guard::check_catalog(&catalog)
            .map_err(|_| AggregateError::AdaptationSourceChanged)?;
        [
            Contract::DataArray,
            Contract::Matrix,
            Contract::DataRecord,
            Contract::Vector,
            Contract::DataChoice,
        ]
        .into_iter()
        .map(|contract| {
            validation::compile_component(&catalog, contract)
                .map(|validator| (contract, validator))
                .map_err(|_| AggregateError::AdaptationSourceChanged)
        })
        .collect()
    })
}

fn correct_count_schema(schema: &mut Value) -> Result<(), AggregateError> {
    let branches = schema
        .pointer_mut("/$defs/AbstractArray/allOf/1/properties/elementCount/oneOf")
        .ok_or(AggregateError::AdaptationSourceChanged)?;
    if *branches
        != json!([
            {"$ref":"basicTypes.json#/$defs/AssociationAttributeGroup"},
            {"$ref":"basicTypes.json#/$defs/ElementCount"}
        ])
    {
        return Err(AggregateError::AdaptationSourceChanged);
    }
    *branches = json!([
        {"$ref":"basicTypes.json#/$defs/AssociationAttributeGroup"},
        {
            "allOf":[{"$ref":"basicTypes.json#/$defs/ElementCount"}],
            "not":{"required":["href"]}
        }
    ]);
    Ok(())
}

/// Inspect only component positions, not arbitrary extension objects. Exact
/// local targets known to be non-Count are inconsistent; absent/ambiguous Count
/// targets remain unresolved for the separate graph/occurrence resolver.
pub(crate) fn references(source: &Value) -> Result<(), ComponentError> {
    let mut components = vec![(source, Vec::new())];
    let mut ids: BTreeMap<&str, Vec<bool>> = BTreeMap::new();
    let mut references = Vec::new();
    while let Some((component, path)) = components.pop() {
        let kind = component.get("type").and_then(Value::as_str);
        if let Some(id) = component.get("id").and_then(Value::as_str) {
            ids.entry(id).or_default().push(kind == Some("Count"));
        }
        if matches!(kind, Some("DataArray" | "Matrix")) {
            if let Some(count) = component.get("elementCount") {
                if count.get("href").is_none()
                    && let Some(id) = count.get("id").and_then(Value::as_str)
                {
                    ids.entry(id).or_default().push(true);
                }
                if let Some(href) = count.get("href").and_then(Value::as_str) {
                    references.push((href, path.clone()));
                }
            }
            if let Some(child) = component.get("elementType") {
                let mut child_path = path.clone();
                child_path.push(0);
                components.push((child, child_path));
            }
        }
        let member = match kind {
            Some("DataRecord") => Some("fields"),
            Some("Vector") => Some("coordinates"),
            Some("DataChoice") => Some("items"),
            _ => None,
        };
        if let Some(member) = member
            && let Some(children) = component.get(member).and_then(Value::as_array)
        {
            for (index, child) in children.iter().enumerate() {
                let mut child_path = path.clone();
                child_path.push(index);
                components.push((child, child_path));
            }
        }
        if kind == Some("DataChoice")
            && let Some(selector) = component.get("choiceValue")
        {
            components.push((selector, path));
        }
    }
    for (reference, path) in references {
        if let Some(id) = reference.strip_prefix('#')
            && let Some(targets) = ids.get(id)
            && targets.iter().all(|is_count| !is_count)
        {
            let mut error = ComponentError::from(AggregateError::CountReference);
            error.path = path;
            return Err(error);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
