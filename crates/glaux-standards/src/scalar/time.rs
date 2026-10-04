//! SWE Time coordinates with explicit reference context, not a conversion engine.
use std::{cmp::Ordering, collections::BTreeMap};

use glaux_domain::{
    numeric::{CountValue, NumericValue},
    scalar::{
        BoundTimeValue, CalendarTime, ComponentMetadata, ScalarComponent, ScalarValue,
        TimeComponent, TimeConstraint, TimeFrame, TimePosition, TimeReference,
    },
    temporal::ExactInstant,
};
use serde_json::{Value, value::RawValue};

use super::{CheckedScalarValue, CodeSpaceCheck, ScalarContract, ScalarError, UnitReferenceCheck};

pub(super) const GREGORIAN: &str = "http://www.opengis.net/def/uom/ISO-8601/0/Gregorian";
const UTC: &str = "http://www.opengis.net/def/trs/BIPM/0/UTC";

fn utc(frame: &TimeFrame) -> bool {
    matches!(frame, TimeFrame::DefaultUtc)
        || matches!(frame, TimeFrame::Declared(uri) if uri == UTC)
}

fn calendar(text: &str, frame: &TimeFrame) -> Result<CalendarTime, ScalarError> {
    // The existing parser validates Gregorian fields and preserves every digit.
    // Its UTC leap table cannot establish leap placement in an unfamiliar frame.
    if !utc(frame) && text.as_bytes().get(17..19) == Some(b"60") {
        return Err(ScalarError::UnsupportedTimeMeaning);
    }
    let parsed = ExactInstant::parse_rfc3339(text).map_err(ScalarError::Time)?;
    Ok(if utc(frame) {
        CalendarTime::Utc(parsed)
    } else {
        CalendarTime::Unresolved(text.to_owned())
    })
}

fn calendar_encoding(reference: &TimeReference) -> bool {
    reference.uom.href.as_deref() == Some(GREGORIAN)
}

pub(super) fn compile(
    metadata: ComponentMetadata,
    source: &Value,
    input: &[u8],
) -> Result<ScalarContract, ScalarError> {
    let frame = metadata
        .reference_frame
        .clone()
        .map_or(TimeFrame::DefaultUtc, TimeFrame::Declared);
    let uom = super::numeric::unit(source)?;
    if let Some(code) = &uom.code {
        if uom.href.as_deref() == Some(GREGORIAN) {
            return Err(ScalarError::Metadata);
        }
        crate::units::validate_time_code(code).map_err(ScalarError::Unit)?;
    }
    let origin = super::optional_string(source, "referenceTime")?
        .map(|text| calendar(&text, &frame))
        .transpose()?;
    let local_frame = super::optional_string(source, "localFrame")?;
    if let Some(uri) = &local_frame {
        super::check_format(uri, "uri")?;
        let effective_frame = match &frame {
            TimeFrame::DefaultUtc => UTC,
            TimeFrame::Declared(uri) => uri,
        };
        if uri == effective_frame {
            return Err(ScalarError::Metadata);
        }
    }
    let reference = TimeReference {
        frame,
        origin,
        local_frame,
        uom,
    };
    let constraint = constraint(source, &reference)?;
    let raw: BTreeMap<String, Box<RawValue>> =
        serde_json::from_slice(input).map_err(|_| ScalarError::Structure)?;
    let value = source
        .get("value")
        .map(|value| position(value, raw.get("value").map(|v| v.get()), &reference, false))
        .transpose()?;
    let component = TimeComponent {
        metadata,
        reference,
        constraint,
        value,
    };
    if let Some(value) = source.get("value") {
        check_value(&component, value, raw.get("value").map(|v| v.get()))?;
    }
    Ok(ScalarContract {
        component: ScalarComponent::Time(Box::new(component)),
        source: input.to_vec(),
        tokens: None,
    })
}

fn position(
    value: &Value,
    raw: Option<&str>,
    reference: &TimeReference,
    constraint_bound: bool,
) -> Result<TimePosition, ScalarError> {
    if calendar_encoding(reference) {
        let text = value.as_str().ok_or(ScalarError::ValueType)?;
        // Published JSON AllowedTimes example permits infinite bounds around
        // calendar values; req56 excludes specials as ISO-formatted data values.
        if constraint_bound && matches!(text, "Infinity" | "+Infinity" | "-Infinity") {
            return super::numeric::number(value, raw).map(TimePosition::Numeric);
        }
        if NumericValue::from_swe_special(text).is_ok() {
            return Err(ScalarError::ValueType);
        }
        calendar(text, &reference.frame).map(TimePosition::Calendar)
    } else {
        super::numeric::number(value, raw).map(TimePosition::Numeric)
    }
}

fn compare(left: &TimePosition, right: &TimePosition) -> Result<Option<Ordering>, ScalarError> {
    use TimePosition::{Calendar, Numeric};
    match (left, right) {
        (Numeric(a), Numeric(b)) => Ok(a.partial_cmp(b)),
        (Calendar(CalendarTime::Utc(a)), Calendar(CalendarTime::Utc(b))) => Ok(Some(a.cmp(b))),
        (Calendar(CalendarTime::Unresolved(a)), Calendar(CalendarTime::Unresolved(b)))
            if a == b =>
        {
            Ok(Some(Ordering::Equal))
        }
        (Numeric(NumericValue::NegativeInfinity), Calendar(_))
        | (Calendar(_), Numeric(NumericValue::PositiveInfinity)) => Ok(Some(Ordering::Less)),
        (Numeric(NumericValue::PositiveInfinity), Calendar(_))
        | (Calendar(_), Numeric(NumericValue::NegativeInfinity)) => Ok(Some(Ordering::Greater)),
        _ => Err(ScalarError::UnsupportedTimeMeaning),
    }
}

fn constraint(
    source: &Value,
    reference: &TimeReference,
) -> Result<Option<TimeConstraint>, ScalarError> {
    let Some(source) = source.get("constraint") else {
        return Ok(None);
    };
    let mut values = Vec::new();
    if let Some(items) = source.get("values").and_then(Value::as_array) {
        for item in items {
            values.push(position(item, None, reference, true)?);
        }
    }
    let mut intervals = Vec::new();
    if let Some(items) = source.get("intervals").and_then(Value::as_array) {
        for item in items {
            let pair = item.as_array().ok_or(ScalarError::Constraint)?;
            if pair.len() != 2 {
                return Err(ScalarError::Constraint);
            }
            let low = position(&pair[0], None, reference, true)?;
            let high = position(&pair[1], None, reference, true)?;
            if !matches!(
                compare(&low, &high)?,
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
            if calendar_encoding(reference) {
                return Err(ScalarError::UnsupportedTimeMeaning);
            }
            let NumericValue::Finite(value) = super::numeric::number(v, None)? else {
                return Err(ScalarError::Constraint);
            };
            let n = CountValue::try_from(value)
                .and_then(|v| v.try_to_u64())
                .map_err(ScalarError::Numeric)?;
            if !(1..=40).contains(&n) {
                return Err(ScalarError::Constraint);
            }
            Ok(n as u8)
        })
        .transpose()?;
    Ok(Some(TimeConstraint {
        values,
        intervals,
        significant_figures,
    }))
}

pub(super) fn check_value(
    component: &TimeComponent,
    value: &Value,
    raw: Option<&str>,
) -> Result<CheckedScalarValue, ScalarError> {
    let position = position(value, raw, &component.reference, false)?;
    if let Some(constraint) = &component.constraint {
        let mut matched = false;
        for allowed in &constraint.values {
            if matches!(
                (allowed, &position),
                (
                    TimePosition::Numeric(NumericValue::NaN),
                    TimePosition::Numeric(NumericValue::NaN)
                )
            ) || compare(allowed, &position)? == Some(Ordering::Equal)
            {
                matched = true;
            }
        }
        for [low, high] in &constraint.intervals {
            if matches!(
                compare(low, &position)?,
                Some(Ordering::Less | Ordering::Equal)
            ) && matches!(
                compare(&position, high)?,
                Some(Ordering::Less | Ordering::Equal)
            ) {
                matched = true;
            }
        }
        if !matched {
            return Err(ScalarError::ConstraintViolation);
        }
        if let (Some(maximum), TimePosition::Numeric(NumericValue::Finite(value))) =
            (constraint.significant_figures, &position)
        {
            let text = value
                .decimal_lexeme()
                .ok_or(ScalarError::UnsupportedTimeMeaning)?;
            if super::numeric::significant_digits(text) > usize::from(maximum) {
                return Err(ScalarError::ConstraintViolation);
            }
        }
    }
    let unit_reference = if calendar_encoding(&component.reference) {
        UnitReferenceCheck::CalendarEncoding
    } else {
        match &component.reference.uom.href {
            Some(uri) => UnitReferenceCheck::Unresolved(uri.clone()),
            None => UnitReferenceCheck::CodeValidated,
        }
    };
    Ok(CheckedScalarValue {
        value: ScalarValue::Time(Box::new(BoundTimeValue {
            position,
            reference: component.reference.clone(),
        })),
        code_space: CodeSpaceCheck::NotApplicable,
        unit_reference,
    })
}
