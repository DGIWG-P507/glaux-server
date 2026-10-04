//! Typed nil declarations, with no implicit nulls or reason-URI resolution.
use std::collections::BTreeMap;

use glaux_domain::{
    numeric::NumericValue,
    scalar::{NilDeclaration, ScalarComponent, ScalarValue, TimePosition},
};
use serde_json::{Value, value::RawValue};

use super::{MAX_NIL_DECLARATIONS, ScalarContract, ScalarError, ValueContext};

pub(super) fn compile(
    contract: &ScalarContract,
    source: &Value,
    raw: &BTreeMap<String, Box<RawValue>>,
) -> Result<Vec<NilDeclaration>, ScalarError> {
    let Some(entries) = source.get("nilValues") else {
        return Ok(Vec::new());
    };
    if matches!(contract.component, ScalarComponent::Boolean { .. }) {
        return Err(ScalarError::NilDeclaration);
    }
    let entries = entries.as_array().ok_or(ScalarError::NilDeclaration)?;
    if entries.is_empty() {
        return Err(ScalarError::NilDeclaration);
    }
    if entries.len() > MAX_NIL_DECLARATIONS {
        return Err(ScalarError::NilLimit);
    }
    let raw_entries: Vec<Box<RawValue>> = serde_json::from_str(
        raw.get("nilValues")
            .ok_or(ScalarError::NilDeclaration)?
            .get(),
    )
    .map_err(|_| ScalarError::NilDeclaration)?;
    if entries.len() != raw_entries.len() {
        return Err(ScalarError::NilDeclaration);
    }
    let mut declarations: Vec<NilDeclaration> = Vec::with_capacity(entries.len());
    for (entry, raw_entry) in entries.iter().zip(&raw_entries) {
        let reason = entry
            .get("reason")
            .and_then(Value::as_str)
            .ok_or(ScalarError::NilDeclaration)?;
        super::check_format(reason, "uri").map_err(|_| ScalarError::NilDeclaration)?;
        let raw_members: BTreeMap<String, Box<RawValue>> =
            serde_json::from_str(raw_entry.get()).map_err(|_| ScalarError::NilDeclaration)?;
        let value = entry.get("value").ok_or(ScalarError::NilDeclaration)?;
        let checked = contract.check_parsed_value(
            value,
            raw_members.get("value").map(|value| value.get()),
            ValueContext::NilDeclaration,
            false,
        )?;
        if declarations
            .iter()
            .any(|declaration| same_value(&declaration.value, &checked.value))
        {
            // Even repeated identical reasons are rejected. One typed sentinel
            // has exactly one declaration; declaration order cannot choose it.
            return Err(ScalarError::DuplicateNilValue);
        }
        declarations.push(NilDeclaration {
            value: checked.value,
            reason: reason.to_owned(),
        });
    }
    Ok(declarations)
}

pub(super) fn same_value(left: &ScalarValue, right: &ScalarValue) -> bool {
    // IEEE NaN stays unequal in the numeric type; only reserved-token matching
    // recognizes the explicit NaN state. No ordinary number is turned into nil.
    match (left, right) {
        (ScalarValue::Quantity(NumericValue::NaN), ScalarValue::Quantity(NumericValue::NaN)) => true,
        (ScalarValue::Time(left), ScalarValue::Time(right))
            if matches!(left.position, TimePosition::Numeric(NumericValue::NaN))
                && matches!(right.position, TimePosition::Numeric(NumericValue::NaN)) =>
        {
            left.reference == right.reference
        }
        _ => left == right,
    }
}
