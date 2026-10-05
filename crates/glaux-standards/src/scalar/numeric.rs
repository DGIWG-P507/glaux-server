//! Local numeric component semantics; no payload codec or unit conversion.
use std::cmp::Ordering;

use glaux_domain::{
    numeric::{CountValue, ExactNumber, NumericValue},
    scalar::{ComponentMetadata, NumericConstraint, ScalarComponent, ScalarValue, UnitReference},
};
use serde_json::Value;

use super::{CheckedScalarValue, CodeSpaceCheck, ScalarContract, ScalarError, UnitReferenceCheck};
use crate::validation::Contract;

pub(super) fn compile(
    kind: Contract,
    metadata: ComponentMetadata,
    source: &Value,
    input: &[u8],
) -> Result<ScalarContract, ScalarError> {
    let count = kind == Contract::Count;
    let constraint = constraint(source, count)?;
    let component = if count {
        ScalarComponent::Count {
            metadata,
            constraint,
            value: None,
        }
    } else {
        ScalarComponent::Quantity {
            metadata,
            constraint,
            uom: unit(source)?,
            value: None,
        }
    };
    Ok(ScalarContract {
        component,
        source: input.to_vec(),
        tokens: None,
        nil_declarations: Vec::new(),
        inline: None,
    })
}

pub(super) fn unit(source: &Value) -> Result<UnitReference, ScalarError> {
    let source = source.get("uom").ok_or(ScalarError::Metadata)?;
    let code = super::optional_string(source, "code")?;
    let href = super::optional_string(source, "href")?;
    if let Some(code) = &code {
        crate::units::validate_code(code).map_err(ScalarError::Unit)?;
    }
    if let Some(href) = &href {
        super::check_format(href, "uri")?;
    }
    Ok(UnitReference {
        label: super::optional_string(source, "label")?,
        symbol: super::optional_string(source, "symbol")?,
        code,
        href,
    })
}

pub(super) fn number(value: &Value, raw: Option<&str>) -> Result<NumericValue, ScalarError> {
    match value {
        Value::Number(value) => ExactNumber::parse_json_number(raw.unwrap_or(value.as_str()))
            .map(NumericValue::Finite)
            .map_err(ScalarError::Numeric),
        Value::String(value) => {
            NumericValue::from_swe_special(value).map_err(|_| ScalarError::ValueType)
        }
        _ => Err(ScalarError::ValueType),
    }
}

fn count_value(value: NumericValue) -> Result<CountValue, ScalarError> {
    let NumericValue::Finite(value) = value else {
        return Err(ScalarError::ValueType);
    };
    CountValue::try_from(value).map_err(ScalarError::Numeric)
}

/// ElementCount has optional identification metadata, unlike a full Count.
/// Reuse exact numeric/constraint rules without inventing required metadata.
pub(crate) fn element_count(
    source: &Value,
    raw_value: Option<&str>,
) -> Result<(Option<NumericConstraint>, Option<CountValue>), ScalarError> {
    let constraint = constraint(source, true)?;
    let value = source
        .get("value")
        .map(|value| -> Result<CountValue, ScalarError> {
            let numeric = number(value, raw_value)?;
            let count = count_value(numeric.clone())?;
            if let Some(constraint) = &constraint {
                check_constraint(constraint, &numeric)?;
            }
            Ok(count)
        })
        .transpose()?;
    Ok((constraint, value))
}

fn constraint(source: &Value, count: bool) -> Result<Option<NumericConstraint>, ScalarError> {
    let Some(source) = source.get("constraint") else {
        return Ok(None);
    };
    let values: Vec<NumericValue> = match source.get("values").and_then(Value::as_array) {
        Some(values) => values
            .iter()
            .map(|v| number(v, None))
            .collect::<Result<_, _>>()?,
        None => Vec::new(),
    };
    let mut intervals = Vec::new();
    if let Some(ranges) = source.get("intervals").and_then(Value::as_array) {
        for range in ranges {
            let range = range.as_array().ok_or(ScalarError::Constraint)?;
            if range.len() != 2 {
                return Err(ScalarError::Constraint);
            }
            let low = number(&range[0], None)?;
            let high = number(&range[1], None)?;
            if !matches!(
                low.partial_cmp(&high),
                Some(Ordering::Less | Ordering::Equal)
            ) {
                return Err(ScalarError::Constraint);
            }
            intervals.push([low, high]);
        }
    }
    let significant_figures = source
        .get("significantFigures")
        .map(|v| {
            let value = count_value(number(v, None)?)?
                .try_to_u64()
                .map_err(ScalarError::Numeric)?;
            if !(1..=40).contains(&value) || count {
                return Err(ScalarError::Constraint);
            }
            Ok(value as u8)
        })
        .transpose()?;
    if count {
        for value in values.iter().chain(intervals.iter().flatten()) {
            count_value(value.clone()).map_err(|_| ScalarError::Constraint)?;
        }
    }
    Ok(Some(NumericConstraint {
        values,
        intervals,
        significant_figures,
    }))
}

pub(super) fn check_value(
    component: &ScalarComponent,
    value: &Value,
    raw: Option<&str>,
    enforce_constraints: bool,
) -> Result<CheckedScalarValue, ScalarError> {
    let numeric = number(value, raw)?;
    let (value, constraint, unit_reference) = match component {
        ScalarComponent::Count { constraint, .. } => (
            ScalarValue::Count(count_value(numeric.clone())?),
            constraint,
            UnitReferenceCheck::NotApplicable,
        ),
        ScalarComponent::Quantity {
            constraint, uom, ..
        } => (
            ScalarValue::Quantity(numeric.clone()),
            constraint,
            match &uom.href {
                Some(uri) => UnitReferenceCheck::Unresolved(uri.clone()),
                None => UnitReferenceCheck::CodeValidated,
            },
        ),
        _ => return Err(ScalarError::UnsupportedComponent),
    };
    if enforce_constraints && let Some(constraint) = constraint {
        check_constraint(constraint, &numeric)?;
    }
    Ok(CheckedScalarValue {
        value,
        code_space: CodeSpaceCheck::NotApplicable,
        unit_reference,
        nil_reason: None,
    })
}

fn check_constraint(
    constraint: &NumericConstraint,
    value: &NumericValue,
) -> Result<(), ScalarError> {
    // Named NaN token membership is explicit; NumericValue's IEEE equality and
    // partial ordering are unchanged. NaN never belongs to an ordered interval.
    let enumerated = constraint.values.iter().any(|allowed| {
        allowed == value || matches!((allowed, value), (NumericValue::NaN, NumericValue::NaN))
    });
    let in_interval = constraint
        .intervals
        .iter()
        .any(|[low, high]| low <= value && value <= high);
    if !(enumerated || in_interval) {
        return Err(ScalarError::ConstraintViolation);
    }
    if let (Some(maximum), NumericValue::Finite(value)) = (constraint.significant_figures, value) {
        let source = value
            .decimal_lexeme()
            .ok_or(ScalarError::UnsupportedFeature)?;
        if significant_digits(source) > usize::from(maximum) {
            return Err(ScalarError::ConstraintViolation);
        }
    }
    Ok(())
}

pub(super) fn significant_digits(source: &str) -> usize {
    let mantissa = source.split(['e', 'E']).next().unwrap_or(source);
    let digits: Vec<_> = mantissa.bytes().filter(u8::is_ascii_digit).collect();
    match digits.iter().position(|digit| *digit != b'0') {
        Some(first) => digits.len() - first,
        // Explicit all-zero convention: fractional places (at least one), not
        // the exponent; no rounding or synthetic precision is applied.
        None => mantissa
            .split_once('.')
            .map_or(1, |(_, fraction)| fraction.len().max(1)),
    }
}
