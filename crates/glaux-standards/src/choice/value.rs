use glaux_domain::{
    aggregate::AggregateComponent,
    scalar::ScalarComponent,
};

use super::{
    CheckedChoiceValue, CheckedComponentValue, CheckedNamedValue, ComponentError,
    ComponentErrorKind, ComponentValue, NamedValue,
};
use crate::{aggregate::{AggregateContract, NamedContract}, validation};

pub(super) fn check(
    contract: &AggregateContract,
    input: &ComponentValue<'_>,
) -> Result<CheckedComponentValue, ComponentError> {
    budget(input)?;
    aggregate(contract, input)
}

// This checks the complete borrowed model before semantic recursion/output.
// Raw leaf JSON additionally uses the existing lexical and numeric budgets.
fn budget(input: &ComponentValue<'_>) -> Result<(), ComponentError> {
    let limit = || ComponentError::new(ComponentErrorKind::Limit);
    let mut pending = vec![(input, 1_usize)];
    let mut nodes = 0_usize;
    let mut bytes = 0_usize;
    while let Some((value, depth)) = pending.pop() {
        nodes = nodes.checked_add(1).ok_or_else(limit)?;
        if nodes > validation::MAX_NODES || depth > validation::MAX_DEPTH {
            return Err(limit());
        }
        match value {
            ComponentValue::ScalarJson(raw) | ComponentValue::RangeJson(raw) => {
                bytes = bytes.checked_add(raw.len()).ok_or_else(limit)?;
            }
            ComponentValue::Record(values)
            | ComponentValue::Vector(values)
            | ComponentValue::Choice(values) => {
                if values.len() > validation::MAX_MEMBERS {
                    return Err(limit());
                }
                for value in *values {
                    if value.name.len() > validation::MAX_STRING_BYTES {
                        return Err(limit());
                    }
                    bytes = bytes.checked_add(value.name.len()).ok_or_else(limit)?;
                    pending.push((&value.value, depth + 1));
                }
            }
        }
        if bytes > validation::MAX_BYTES {
            return Err(limit());
        }
    }
    Ok(())
}

fn aggregate(
    contract: &AggregateContract,
    input: &ComponentValue<'_>,
) -> Result<CheckedComponentValue, ComponentError> {
    match (contract.component(), input) {
        (AggregateComponent::Choice { .. }, ComponentValue::Choice(values)) => {
            if values.len() != 1 {
                return Err(ComponentError::new(ComponentErrorKind::SelectionCardinality));
            }
            let selected = &values[0];
            let (index, child) = contract
                .children()
                .iter()
                .enumerate()
                .find(|(_, child)| child.name() == selected.name)
                .ok_or_else(|| ComponentError::new(ComponentErrorKind::UnknownSelection))?;
            // Identity selects exactly one contract. Never fall back to another
            // arm merely because that arm accepts the supplied value.
            let value = check_child(child, &selected.value).map_err(|error| error.at(index))?;
            Ok(CheckedComponentValue::Choice(Box::new(CheckedChoiceValue {
                name: child.name().to_owned(),
                value,
            })))
        }
        (AggregateComponent::Record { .. }, ComponentValue::Record(values)) => {
            members(contract, values, true).map(CheckedComponentValue::Record)
        }
        (
            AggregateComponent::Vector { reference_frame, local_frame, .. },
            ComponentValue::Vector(values),
        ) => Ok(CheckedComponentValue::Vector {
            reference_frame: reference_frame.clone(),
            local_frame: local_frame.clone(),
            coordinates: members(contract, values, false)?,
        }),
        _ => Err(ComponentError::new(ComponentErrorKind::ValueType)),
    }
}

fn check_child(
    contract: &NamedContract,
    input: &ComponentValue<'_>,
) -> Result<CheckedComponentValue, ComponentError> {
    if let Some(scalar) = contract.scalar() {
        let ComponentValue::ScalarJson(raw) = input else {
            return Err(ComponentError::new(ComponentErrorKind::ValueType));
        };
        return scalar
            .check_value(raw)
            .map(|value| CheckedComponentValue::Scalar(Box::new(value)))
            .map_err(|error| ComponentError::new(ComponentErrorKind::Scalar(error)));
    }
    if let Some(range) = contract.range() {
        let ComponentValue::RangeJson(raw) = input else {
            return Err(ComponentError::new(ComponentErrorKind::ValueType));
        };
        return range
            .check_value(raw)
            .map(|value| CheckedComponentValue::Range(Box::new(value)))
            .map_err(|error| ComponentError::new(ComponentErrorKind::Range(error)));
    }
    if let Some(contract) = contract.aggregate() {
        return aggregate(contract, input);
    }
    Err(ComponentError::new(ComponentErrorKind::ValueType))
}

fn members(
    contract: &AggregateContract,
    values: &[NamedValue<'_>],
    allow_optional: bool,
) -> Result<Vec<CheckedNamedValue>, ComponentError> {
    let mut supplied = vec![None; contract.children().len()];
    for value in values {
        let index = contract
            .children()
            .iter()
            .position(|child| child.name() == value.name)
            .ok_or_else(|| ComponentError::new(ComponentErrorKind::UnknownMember))?;
        if supplied[index].replace(&value.value).is_some() {
            return Err(ComponentError::new(ComponentErrorKind::DuplicateMember).at(index));
        }
    }
    contract
        .children()
        .iter()
        .zip(supplied)
        .enumerate()
        .map(|(index, (child, value))| {
            let value = match value {
                Some(value) => Some(check_child(child, value).map_err(|error| error.at(index))?),
                None if allow_optional && optional(child) => None,
                None => return Err(ComponentError::new(ComponentErrorKind::MissingMember).at(index)),
            };
            Ok(CheckedNamedValue { name: child.name().to_owned(), value })
        })
        .collect()
}

fn optional(contract: &NamedContract) -> bool {
    fn scalar_optional(component: &ScalarComponent) -> bool {
        let metadata = match component {
            ScalarComponent::Boolean { metadata, .. }
            | ScalarComponent::Text { metadata, .. }
            | ScalarComponent::Category { metadata, .. }
            | ScalarComponent::Count { metadata, .. }
            | ScalarComponent::Quantity { metadata, .. } => metadata,
            ScalarComponent::Time(component) => &component.metadata,
        };
        metadata.optional == Some(true)
    }
    if let Some(scalar) = contract.scalar() {
        return scalar_optional(scalar.component());
    }
    if let Some(range) = contract.range() {
        return scalar_optional(&range.component().endpoint);
    }
    contract.aggregate().is_some_and(|contract| {
        let metadata = match contract.component() {
            AggregateComponent::Record { metadata, .. }
            | AggregateComponent::Vector { metadata, .. }
            | AggregateComponent::Choice { metadata, .. } => metadata,
        };
        metadata.optional == Some(true)
    })
}
