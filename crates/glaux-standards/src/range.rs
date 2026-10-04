//! Bounded SWE extent contracts; source validation and local meaning are separate.
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
};

use glaux_domain::{
    numeric::NumericValue,
    range::{RangeComponent, RangeEndpoint, RangeKind},
    scalar::{CalendarTime, ScalarComponent, ScalarValue, TimePosition, TokenConstraint},
};
use serde_json::{Value, value::RawValue};

use crate::{
    scalar::{CheckedScalarValue, ScalarContract, ScalarError},
    validation::{self, Contract, StructuralValidator},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RangeError {
    Syntax(validation::Failure),
    Structure,
    UnsupportedComponent,
    Endpoint(ScalarError),
    Cardinality,
    ReversedBounds,
    CategoryOrder,
    AdaptationSourceChanged,
}

/// Caller-supplied, local ordering evidence, not a fetched dictionary or a claim
/// that an AllowedTokens list or lexical sort defines the category order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CategoryOrder {
    pub code_space: Option<String>,
    pub ascending_tokens: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CountNilSchema {
    #[default]
    Original,
    /// Explicitly select the documented NilValuesText -> NilValuesInteger seam.
    IntegerNilCorrection,
}

#[derive(Default)]
pub struct RangeOptions<'a> {
    pub category_order: Option<&'a CategoryOrder>,
    pub count_nil_schema: CountNilSchema,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceValidation {
    Original,
    /// The original document failed the original schema; only the named local
    /// correction passed. This is never an unmodified upstream-schema pass.
    CountIntegerNilCorrection,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OrderCheck {
    Established,
    UnresolvedCategory,
    UnresolvedTimeFrame,
    UnorderedSpecial,
    NilEndpoint,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CheckedRangeValue {
    pub endpoints: [CheckedScalarValue; 2],
    pub order: OrderCheck,
}

pub struct RangeContract {
    component: RangeComponent,
    endpoint: ScalarContract,
    source: Vec<u8>,
    source_validation: SourceValidation,
    category_order: Option<CategoryOrder>,
    inline: Option<CheckedRangeValue>,
}

impl RangeContract {
    pub fn compile(
        validator: &StructuralValidator,
        input: &[u8],
        options: RangeOptions<'_>,
    ) -> Result<Self, RangeError> {
        let source = validation::parse(input).map_err(RangeError::Syntax)?;
        let (kind, original, scalar) = match source.get("type").and_then(Value::as_str) {
            Some("CategoryRange") => (RangeKind::Category, Contract::CategoryRange, "Category"),
            Some("CountRange") => (RangeKind::Count, Contract::CountRange, "Count"),
            Some("QuantityRange") => (RangeKind::Quantity, Contract::QuantityRange, "Quantity"),
            Some("TimeRange") => (RangeKind::Time, Contract::TimeRange, "Time"),
            _ => return Err(RangeError::UnsupportedComponent),
        };
        let source_validation = match validator.validate(original, input) {
            Ok(()) => SourceValidation::Original,
            Err(_)
                if kind == RangeKind::Count
                    && options.count_nil_schema == CountNilSchema::IntegerNilCorrection =>
            {
                validate_count_correction(&source)?;
                SourceValidation::CountIntegerNilCorrection
            }
            Err(_) => return Err(RangeError::Structure),
        };
        // Derive only the shared endpoint descriptor after validating the range.
        // RawValue preserves -0/exponents in nil declarations and constraints.
        let mut raw: BTreeMap<String, Box<RawValue>> =
            serde_json::from_slice(input).map_err(|_| RangeError::Structure)?;
        let inline = raw.remove("value");
        raw.insert(
            "type".into(),
            RawValue::from_string(format!("\"{scalar}\"")).map_err(|_| RangeError::Structure)?,
        );
        let bytes = serde_json::to_vec(&raw).map_err(|_| RangeError::Structure)?;
        let endpoint = ScalarContract::compile_range_endpoint_descriptor(validator, &bytes)
            .map_err(RangeError::Endpoint)?;
        let category_order = validate_category_order(kind, &endpoint, options.category_order)?;
        let component = RangeComponent {
            kind,
            endpoint: endpoint.component().clone(),
            nil_values: endpoint.nil_declarations().to_vec(),
            value: None,
        };
        let mut compiled = Self {
            component,
            endpoint,
            source: input.to_vec(),
            source_validation,
            category_order,
            inline: None,
        };
        if let Some(inline) = inline {
            let checked = compiled.check_value(inline.get().as_bytes())?;
            compiled.component.value =
                Some(checked.endpoints.clone().map(|endpoint| RangeEndpoint {
                    value: endpoint.value,
                    nil_reason: endpoint.nil_reason,
                }));
            compiled.inline = Some(checked);
        }
        Ok(compiled)
    }

    pub fn component(&self) -> &RangeComponent {
        &self.component
    }
    pub fn source(&self) -> &[u8] {
        &self.source
    }
    pub fn source_validation(&self) -> SourceValidation {
        self.source_validation
    }
    pub fn inline_value(&self) -> Option<&CheckedRangeValue> {
        self.inline.as_ref()
    }

    /// Checks a supplied pair. No value is a separate Option state; JSON null is
    /// not a pair or a declaration of a nil reason. Payload framing is separate.
    pub fn check_value(&self, input: &[u8]) -> Result<CheckedRangeValue, RangeError> {
        validation::parse(input).map_err(RangeError::Syntax)?;
        let values: Vec<Box<RawValue>> =
            serde_json::from_slice(input).map_err(|_| RangeError::Cardinality)?;
        if values.len() != 2 {
            return Err(RangeError::Cardinality);
        }
        let endpoints = [
            self.endpoint
                .check_range_endpoint(values[0].get().as_bytes())
                .map_err(RangeError::Endpoint)?,
            self.endpoint
                .check_range_endpoint(values[1].get().as_bytes())
                .map_err(RangeError::Endpoint)?,
        ];
        // Category evidence still applies to an ordinary bound paired with nil.
        if let Some(order) = &self.category_order {
            for endpoint in &endpoints {
                if endpoint.nil_reason.is_none()
                    && let ScalarValue::Category(value) = &endpoint.value
                    && !order.ascending_tokens.contains(value)
                {
                    return Err(RangeError::CategoryOrder);
                }
            }
        }
        let order = if endpoints
            .iter()
            .any(|endpoint| endpoint.nil_reason.is_some())
        {
            OrderCheck::NilEndpoint
        } else {
            self.check_order(&endpoints[0].value, &endpoints[1].value)?
        };
        Ok(CheckedRangeValue { endpoints, order })
    }

    fn check_order(&self, low: &ScalarValue, high: &ScalarValue) -> Result<OrderCheck, RangeError> {
        let ordering = match (low, high) {
            (ScalarValue::Count(a), ScalarValue::Count(b)) => Some(a.cmp(b)),
            (ScalarValue::Quantity(a), ScalarValue::Quantity(b)) => a.partial_cmp(b),
            (ScalarValue::Category(a), ScalarValue::Category(b)) => {
                let Some(order) = &self.category_order else {
                    return Ok(OrderCheck::UnresolvedCategory);
                };
                let a = order
                    .ascending_tokens
                    .iter()
                    .position(|v| v == a)
                    .ok_or(RangeError::CategoryOrder)?;
                let b = order
                    .ascending_tokens
                    .iter()
                    .position(|v| v == b)
                    .ok_or(RangeError::CategoryOrder)?;
                Some(a.cmp(&b))
            }
            (ScalarValue::Time(a), ScalarValue::Time(b)) => match (&a.position, &b.position) {
                (TimePosition::Numeric(a), TimePosition::Numeric(b)) => a.partial_cmp(b),
                (
                    TimePosition::Calendar(CalendarTime::Utc(a)),
                    TimePosition::Calendar(CalendarTime::Utc(b)),
                ) => Some(a.cmp(b)),
                (
                    TimePosition::Calendar(CalendarTime::Unresolved(a)),
                    TimePosition::Calendar(CalendarTime::Unresolved(b)),
                ) if a == b => Some(Ordering::Equal),
                (
                    TimePosition::Numeric(NumericValue::NegativeInfinity),
                    TimePosition::Calendar(_),
                )
                | (
                    TimePosition::Calendar(_),
                    TimePosition::Numeric(NumericValue::PositiveInfinity),
                ) => Some(Ordering::Less),
                (
                    TimePosition::Numeric(NumericValue::PositiveInfinity),
                    TimePosition::Calendar(_),
                )
                | (
                    TimePosition::Calendar(_),
                    TimePosition::Numeric(NumericValue::NegativeInfinity),
                ) => Some(Ordering::Greater),
                (TimePosition::Numeric(NumericValue::NaN), _)
                | (_, TimePosition::Numeric(NumericValue::NaN)) => None,
                _ => return Ok(OrderCheck::UnresolvedTimeFrame),
            },
            _ => return Err(RangeError::Endpoint(ScalarError::ValueType)),
        };
        match ordering {
            Some(Ordering::Greater) => Err(RangeError::ReversedBounds),
            Some(_) => Ok(OrderCheck::Established),
            None => Ok(OrderCheck::UnorderedSpecial),
        }
    }
}

fn validate_category_order(
    kind: RangeKind,
    endpoint: &ScalarContract,
    evidence: Option<&CategoryOrder>,
) -> Result<Option<CategoryOrder>, RangeError> {
    let Some(evidence) = evidence else {
        return Ok(None);
    };
    let ScalarComponent::Category {
        code_space,
        constraint,
        ..
    } = endpoint.component()
    else {
        return Err(RangeError::CategoryOrder);
    };
    if kind != RangeKind::Category
        || *code_space != evidence.code_space
        || evidence.ascending_tokens.is_empty()
        || evidence.ascending_tokens.len() > validation::MAX_NODES
        || evidence
            .ascending_tokens
            .iter()
            .any(|v| v.is_empty() || v.len() > validation::MAX_STRING_BYTES)
        || evidence
            .ascending_tokens
            .iter()
            .map(String::len)
            .sum::<usize>()
            > validation::MAX_BYTES
        || evidence
            .ascending_tokens
            .iter()
            .collect::<BTreeSet<_>>()
            .len()
            != evidence.ascending_tokens.len()
    {
        return Err(RangeError::CategoryOrder);
    }
    if let Some(TokenConstraint::Values(values)) = constraint
        && values.iter().any(|value| {
            !evidence.ascending_tokens.contains(value)
                && !endpoint
                    .nil_declarations()
                    .iter()
                    .any(|nil| matches!(&nil.value, ScalarValue::Category(token) if token == value))
        })
    {
        return Err(RangeError::CategoryOrder);
    }
    Ok(Some(evidence.clone()))
}

// This is a separate local catalog, never a repair of the packaged original.
// Callers must opt in; unchanged original validation remains available above.
fn validate_count_correction(source: &Value) -> Result<(), RangeError> {
    const URI: &str = "https://schemas.opengis.net/sweCommon/3.0/json/CountRange.json";
    let mut catalog = validation::catalog().map_err(|_| RangeError::AdaptationSourceChanged)?;
    let schema = catalog
        .get_mut(URI)
        .ok_or(RangeError::AdaptationSourceChanged)?;
    correct_count_schema(schema)?;
    crate::schema_guard::check_catalog(&catalog)
        .map_err(|_| RangeError::AdaptationSourceChanged)?;
    let corrected =
        validation::compile(&catalog, URI).map_err(|_| RangeError::AdaptationSourceChanged)?;
    if corrected.is_valid(source) {
        Ok(())
    } else {
        Err(RangeError::Structure)
    }
}

fn correct_count_schema(schema: &mut Value) -> Result<(), RangeError> {
    let reference = schema
        .pointer_mut("/allOf/1/properties/nilValues/$ref")
        .ok_or(RangeError::AdaptationSourceChanged)?;
    if reference.as_str() != Some("basicTypes.json#/$defs/NilValuesText") {
        return Err(RangeError::AdaptationSourceChanged);
    }
    *reference = Value::String("basicTypes.json#/$defs/NilValuesInteger".into());
    Ok(())
}

#[cfg(test)]
mod tests;
