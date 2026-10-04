//! Bounded Boolean/Text/Category/Count/Quantity/Time contract compilation.
//!
//! Original schema validation precedes local meaning checks. Category dictionary
//! membership is explicitly unresolved; neither definitions nor code spaces are
//! fetched. This is not an observation codec or a claim of full SWE conformance.
use glaux_domain::scalar::{ComponentMetadata, ScalarComponent, ScalarValue, TokenConstraint};
use serde_json::{Value, json};

use crate::validation::{self, Contract, StructuralValidator};
use glaux_domain::numeric::NumericError;

mod numeric;
mod time;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScalarError {
    Syntax(validation::Failure),
    Structure,
    UnsupportedComponent,
    UnsupportedFeature,
    Metadata,
    CategoryWithoutEnumeration,
    Constraint,
    ValueType,
    ConstraintViolation,
    Numeric(NumericError),
    Unit(crate::units::UnitError),
    Time(glaux_domain::temporal::TimeError),
    UnsupportedTimeMeaning,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CodeSpaceCheck {
    NotApplicable,
    /// Both value membership and the allowed-list subset need an external
    /// vocabulary authority. A local constraint pass proves neither.
    Unresolved(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UnitReferenceCheck {
    NotApplicable,
    CodeValidated,
    CalendarEncoding,
    /// URI syntax was checked, not dictionary meaning or agreement with a code.
    Unresolved(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct CheckedScalarValue {
    pub value: ScalarValue,
    pub code_space: CodeSpaceCheck,
    pub unit_reference: UnitReferenceCheck,
}

/// Immutable typed description plus the exact source artifact and local checks.
pub struct ScalarContract {
    component: ScalarComponent,
    source: Vec<u8>,
    tokens: Option<jsonschema::Validator>,
}

impl ScalarContract {
    pub fn compile(validator: &StructuralValidator, input: &[u8]) -> Result<Self, ScalarError> {
        let source = validation::parse(input).map_err(ScalarError::Syntax)?;
        let kind = match source.get("type").and_then(Value::as_str) {
            Some("Boolean") => Contract::Boolean,
            Some("Text") => Contract::Text,
            Some("Category") => Contract::Category,
            Some("Count") => Contract::Count,
            Some("Quantity") => Contract::Quantity,
            Some("Time") => Contract::Time,
            _ => return Err(ScalarError::UnsupportedComponent),
        };
        validator
            .validate(kind, input)
            .map_err(|_| ScalarError::Structure)?;
        // Known semantics owned by later leaves must not disappear as extensions.
        // This does not claim the open upstream Boolean schema forbids constraint.
        if source.get("nilValues").is_some()
            || source.get("quality").is_some()
            || (kind == Contract::Boolean && source.get("constraint").is_some())
        {
            return Err(ScalarError::UnsupportedFeature);
        }
        let metadata = metadata(&source)?;
        if kind == Contract::Time {
            return time::compile(metadata, &source, input);
        }
        if matches!(kind, Contract::Count | Contract::Quantity) {
            return numeric::compile(kind, metadata, &source, input);
        }
        let constraint = token_constraint(&source)?;
        let tokens = match &constraint {
            Some(TokenConstraint::Values(values)) => Some(local_validator(&json!({
                "type": "string", "enum": values
            }))?),
            Some(TokenConstraint::Pattern(pattern)) => {
                supported_pattern(pattern)?;
                Some(local_validator(
                    &json!({"type": "string", "pattern": pattern}),
                )?)
            }
            None => None,
        };
        let component = match kind {
            Contract::Boolean => ScalarComponent::Boolean {
                metadata,
                value: source.get("value").and_then(Value::as_bool),
            },
            Contract::Text => ScalarComponent::Text {
                metadata,
                constraint,
                value: optional_string(&source, "value")?,
            },
            Contract::Category => {
                let code_space = optional_string(&source, "codeSpace")?;
                if code_space.is_none() && !matches!(constraint, Some(TokenConstraint::Values(_))) {
                    return Err(ScalarError::CategoryWithoutEnumeration);
                }
                if let Some(uri) = &code_space {
                    check_format(uri, "uri")?;
                }
                ScalarComponent::Category {
                    metadata,
                    code_space,
                    constraint,
                    value: optional_string(&source, "value")?,
                }
            }
            _ => return Err(ScalarError::UnsupportedComponent),
        };
        let compiled = Self {
            component,
            source: input.to_vec(),
            tokens,
        };
        if let Some(value) = source.get("value") {
            compiled.check_parsed_value(value)?;
        }
        Ok(compiled)
    }

    pub fn component(&self) -> &ScalarComponent {
        &self.component
    }

    /// Includes permitted extension members and original whitespace/escaping.
    pub fn source(&self) -> &[u8] {
        &self.source
    }

    /// Empty input/null is not an absent value. Absence belongs to the enclosing
    /// description/record; this method checks an actually supplied JSON value.
    pub fn check_value(&self, input: &[u8]) -> Result<CheckedScalarValue, ScalarError> {
        let value = validation::parse(input).map_err(ScalarError::Syntax)?;
        if let ScalarComponent::Time(component) = &self.component {
            let raw = std::str::from_utf8(input).map_err(|_| ScalarError::ValueType)?;
            return time::check_value(component, &value, Some(raw.trim()));
        }
        if matches!(
            self.component,
            ScalarComponent::Count { .. } | ScalarComponent::Quantity { .. }
        ) {
            let raw = std::str::from_utf8(input).map_err(|_| ScalarError::ValueType)?;
            return numeric::check_value(&self.component, &value, Some(raw.trim()));
        }
        self.check_parsed_value(&value)
    }

    fn check_parsed_value(&self, value: &Value) -> Result<CheckedScalarValue, ScalarError> {
        let (value, code_space) = match &self.component {
            ScalarComponent::Boolean { .. } => (
                ScalarValue::Boolean(value.as_bool().ok_or(ScalarError::ValueType)?),
                CodeSpaceCheck::NotApplicable,
            ),
            ScalarComponent::Text { .. } => {
                let text = value.as_str().ok_or(ScalarError::ValueType)?;
                self.check_tokens(value)?;
                (
                    ScalarValue::Text(text.to_owned()),
                    CodeSpaceCheck::NotApplicable,
                )
            }
            ScalarComponent::Category { code_space, .. } => {
                let text = value.as_str().ok_or(ScalarError::ValueType)?;
                self.check_tokens(value)?; // Category membership must not be bypassed.
                let status = match code_space {
                    Some(uri) => CodeSpaceCheck::Unresolved(uri.clone()),
                    None => CodeSpaceCheck::NotApplicable,
                };
                (ScalarValue::Category(text.to_owned()), status)
            }
            ScalarComponent::Count { .. } | ScalarComponent::Quantity { .. } => {
                return numeric::check_value(&self.component, value, None);
            }
            ScalarComponent::Time(component) => return time::check_value(component, value, None),
        };
        Ok(CheckedScalarValue {
            value,
            code_space,
            unit_reference: UnitReferenceCheck::NotApplicable,
        })
    }

    fn check_tokens(&self, value: &Value) -> Result<(), ScalarError> {
        if self
            .tokens
            .as_ref()
            .is_some_and(|tokens| !tokens.is_valid(value))
        {
            return Err(ScalarError::ConstraintViolation);
        }
        Ok(())
    }
}

fn optional_string(source: &Value, member: &str) -> Result<Option<String>, ScalarError> {
    source
        .get(member)
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or(ScalarError::Metadata)
        })
        .transpose()
}

fn metadata(source: &Value) -> Result<ComponentMetadata, ScalarError> {
    let definition = optional_string(source, "definition")?.ok_or(ScalarError::Metadata)?;
    check_format(&definition, "uri")?;
    let reference_frame = optional_string(source, "referenceFrame")?;
    if let Some(uri) = &reference_frame {
        check_format(uri, "uri-reference")?;
    }
    Ok(ComponentMetadata {
        id: optional_string(source, "id")?,
        definition,
        label: optional_string(source, "label")?.ok_or(ScalarError::Metadata)?,
        description: optional_string(source, "description")?,
        optional: source.get("optional").and_then(Value::as_bool),
        updatable: source.get("updatable").and_then(Value::as_bool),
        reference_frame,
        axis_id: optional_string(source, "axisID")?,
    })
}

fn token_constraint(source: &Value) -> Result<Option<TokenConstraint>, ScalarError> {
    let Some(constraint) = source.get("constraint") else {
        return Ok(None);
    };
    // Explicitly reject ambiguous known members even if an invalid enum branch
    // happens to leave the original open oneOf's pattern branch valid.
    match (constraint.get("values"), constraint.get("pattern")) {
        (Some(values), None) => {
            let values = values.as_array().ok_or(ScalarError::Constraint)?;
            let tokens = values
                .iter()
                .map(|v| v.as_str().map(str::to_owned).ok_or(ScalarError::Constraint))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Some(TokenConstraint::Values(tokens)))
        }
        (None, Some(pattern)) => Ok(Some(TokenConstraint::Pattern(
            pattern.as_str().ok_or(ScalarError::Constraint)?.to_owned(),
        ))),
        _ => Err(ScalarError::Constraint),
    }
}

fn check_format(value: &str, format: &str) -> Result<(), ScalarError> {
    let schema = json!({"type": "string", "format": format});
    let validator = jsonschema::options()
        .with_draft(jsonschema::Draft::Draft202012)
        .should_validate_formats(true)
        .build(&schema)
        .map_err(|_| ScalarError::Metadata)?;
    if validator.is_valid(&Value::String(value.to_owned())) {
        Ok(())
    } else {
        Err(ScalarError::Metadata)
    }
}

fn local_validator(schema: &Value) -> Result<jsonschema::Validator, ScalarError> {
    // Only fixed local enum/pattern schemas reach here. No $ref or caller schema.
    jsonschema::options()
        .with_draft(jsonschema::Draft::Draft202012)
        .with_pattern_options(
            jsonschema::PatternOptions::fancy_regex()
                .backtrack_limit(20_000)
                .size_limit(1_048_576)
                .dfa_size_limit(1_048_576),
        )
        .build(schema)
        .map_err(|_| ScalarError::Constraint)
}

fn supported_pattern(pattern: &str) -> Result<(), ScalarError> {
    // The pinned engine translates basic ECMA character classes, but bypasses
    // translation for lookaround/backreferences. Do not silently reinterpret
    // those, inline flags, word boundaries or Unicode escape/property syntax.
    let mut chars = pattern.chars().peekable();
    let mut in_class = false;
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            let escaped = chars.next().ok_or(ScalarError::Constraint)?;
            if escaped.is_ascii_alphanumeric() && !"dDwWfnrtv".contains(escaped) {
                return Err(ScalarError::UnsupportedFeature);
            }
        } else if ch == '[' {
            if in_class {
                return Err(ScalarError::UnsupportedFeature);
            }
            in_class = true;
        } else if ch == ']' {
            in_class = false;
        } else if in_class && matches!(ch, '&' | '-' | '~') && chars.peek() == Some(&ch) {
            return Err(ScalarError::UnsupportedFeature);
        } else if !in_class && ch == '.' {
            // Rust dot and the pinned \s translation differ from ECMA on
            // several line terminators/space characters. Fail explicitly.
            return Err(ScalarError::UnsupportedFeature);
        } else if !in_class && ch == '(' && chars.peek() == Some(&'?') {
            chars.next();
            if chars.next() != Some(':') {
                return Err(ScalarError::UnsupportedFeature);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod numeric_tests;

#[cfg(test)]
mod time_tests;
