//! Bounded structural validation of fixed original contracts.
//!
//! No caller-selected schema URI, filesystem path or remote retrieval is exposed.
//! Success is structural only; it does not establish semantics or codec support.
use std::collections::{BTreeMap, HashSet};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use serde_json::Value;

include!(concat!(env!("OUT_DIR"), "/corpus.rs"));

const SWE: &str = "https://schemas.opengis.net/sweCommon/3.0/json/";
pub(crate) const PIN: &str = "https://raw.githubusercontent.com/opengeospatial/ogcapi-connected-systems/8e03b236a049849f2ccc24b4fd9fdce5ff69bed2/";
/// Initial resource budgets, not standard-imposed sizes.
pub const MAX_BYTES: usize = 262_144;
pub const MAX_DEPTH: usize = 32;
pub const MAX_NODES: usize = 4096;
pub const MAX_MEMBERS: usize = 512;
pub const MAX_STRING_BYTES: usize = 16_384;

/// Call-site choice, never a schema URI supplied by a request.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Contract {
    Quantity,
    SweRecord,
    PhysicalSystem,
    ObservationSwe,
    CommandSwe,
    JsonEncoding,
    TextEncoding,
    BinaryEncoding,
}

impl Contract {
    fn uri(self) -> String {
        match self {
            Self::Quantity => format!("{SWE}Quantity.json"),
            Self::SweRecord => format!("{SWE}DataRecord.json"),
            Self::PhysicalSystem => format!("{PIN}sensorml/schemas/json/PhysicalSystem.json"),
            Self::ObservationSwe => {
                format!("{PIN}api/part2/openapi/schemas/json/observationSchemaSwe.json")
            }
            Self::CommandSwe => {
                format!("{PIN}api/part2/openapi/schemas/json/commandSchemaSwe.json")
            }
            Self::JsonEncoding => format!("{SWE}encodings.json#/$defs/JSONEncoding"),
            Self::TextEncoding => format!("{SWE}encodings.json#/$defs/TextEncoding"),
            Self::BinaryEncoding => format!("{SWE}encodings.json#/$defs/BinaryEncoding"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Encoding {
    Json,
    Text,
    Binary,
}

/// Deliberately bounded diagnostics: no source document or schema detail leaks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Failure {
    Size,
    Depth,
    Nodes,
    Members,
    String,
    Malformed,
    DuplicateKey,
    Structure,
    EncodingMismatch,
}

struct Frame {
    object: bool,
    key: bool,
    keys: HashSet<String>,
}

/// Parse only after raw-byte and lexical-depth checks; reject duplicate keys.
pub(crate) fn parse(input: &[u8]) -> Result<Value, Failure> {
    if input.len() > MAX_BYTES {
        return Err(Failure::Size);
    }
    let mut frames: Vec<Frame> = Vec::new();
    let mut i = 0;
    while i < input.len() {
        match input[i] {
            b'{' | b'[' => {
                if frames.len() >= MAX_DEPTH {
                    return Err(Failure::Depth);
                }
                frames.push(Frame {
                    object: input[i] == b'{',
                    key: input[i] == b'{',
                    keys: HashSet::new(),
                });
                i += 1;
            }
            b'}' | b']' => {
                let frame = frames.pop().ok_or(Failure::Malformed)?;
                if frame.object != (input[i] == b'}') {
                    return Err(Failure::Malformed);
                }
                i += 1;
            }
            b',' => {
                if let Some(frame) = frames.last_mut() {
                    frame.key = frame.object;
                }
                i += 1;
            }
            b'"' => {
                let start = i;
                i += 1;
                while i < input.len() {
                    if input[i] == b'\\' {
                        i += 2;
                    } else if input[i] == b'"' {
                        i += 1;
                        break;
                    } else {
                        i += 1;
                    }
                }
                if i > input.len() || input.get(i.wrapping_sub(1)) != Some(&b'"') {
                    return Err(Failure::Malformed);
                }
                let text: String =
                    serde_json::from_slice(&input[start..i]).map_err(|_| Failure::Malformed)?;
                if text.len() > MAX_STRING_BYTES {
                    return Err(Failure::String);
                }
                if let Some(frame) = frames.last_mut()
                    && frame.object
                    && frame.key
                {
                    if !frame.keys.insert(text) {
                        return Err(Failure::DuplicateKey);
                    }
                    if frame.keys.len() > MAX_MEMBERS {
                        return Err(Failure::Members);
                    }
                    frame.key = false;
                }
            }
            _ => i += 1,
        }
    }
    if !frames.is_empty() {
        return Err(Failure::Malformed);
    }
    let raw: Box<serde_json::value::RawValue> =
        serde_json::from_slice(input).map_err(|_| Failure::Malformed)?;
    let value = preserve_wire_kind(&raw)?;
    let mut pending = vec![&value];
    let mut nodes = 0;
    while let Some(node) = pending.pop() {
        nodes += 1;
        if nodes > MAX_NODES {
            return Err(Failure::Nodes);
        }
        match node {
            Value::Array(values) => {
                if values.len() > MAX_MEMBERS {
                    return Err(Failure::Members);
                }
                pending.extend(values);
            }
            Value::Object(values) => {
                if values.len() > MAX_MEMBERS {
                    return Err(Failure::Members);
                }
                pending.extend(values.values());
            }
            _ => {}
        }
    }
    Ok(value)
}

// arbitrary_precision's generic Value visitor recognizes an internal map token.
// RawValue keeps actual JSON objects distinct from that internal number token.
// Depth was bounded lexically before this recursion; serde_json owns the grammar.
fn preserve_wire_kind(raw: &serde_json::value::RawValue) -> Result<Value, Failure> {
    let text = raw.get().trim_start();
    match text.as_bytes().first() {
        Some(b'{') => {
            let members: BTreeMap<String, Box<serde_json::value::RawValue>> =
                serde_json::from_str(text).map_err(|_| Failure::Malformed)?;
            members
                .into_iter()
                .map(|(key, value)| Ok((key, preserve_wire_kind(&value)?)))
                .collect::<Result<serde_json::Map<String, Value>, Failure>>()
                .map(Value::Object)
        }
        Some(b'[') => {
            let members: Vec<Box<serde_json::value::RawValue>> =
                serde_json::from_str(text).map_err(|_| Failure::Malformed)?;
            members
                .into_iter()
                .map(|value| preserve_wire_kind(&value))
                .collect::<Result<Vec<Value>, Failure>>()
                .map(Value::Array)
        }
        _ => serde_json::from_str(text).map_err(|_| Failure::Malformed),
    }
}

/// Compiled once at startup from build-embedded reviewed files.
pub struct StructuralValidator {
    validators: BTreeMap<Contract, jsonschema::Validator>,
}

pub(crate) fn catalog() -> Result<BTreeMap<String, Value>, String> {
    DOCUMENTS
        .iter()
        .map(|(uri, text)| {
            serde_json::from_str(text)
                .map(|value| ((*uri).to_owned(), value))
                .map_err(|e| e.to_string())
        })
        .collect()
}

pub(crate) fn compile(catalog: &BTreeMap<String, Value>, uri: &str) -> Result<jsonschema::Validator, String> {
    compile_with_denial(catalog, uri, DenyRetrieval::default())
}

/// The only installed retriever has no I/O or fallback. Its counter lets tests
/// prove that ordinary validation never asks it to resolve instance data.
#[derive(Clone, Default)]
struct DenyRetrieval(Arc<AtomicUsize>);

impl jsonschema::Retrieve for DenyRetrieval {
    fn retrieve(
        &self,
        _uri: &jsonschema::Uri<String>,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Err("schema retrieval denied: embedded resources only".into())
    }
}

fn compile_with_denial(
    catalog: &BTreeMap<String, Value>,
    uri: &str,
    deny: DenyRetrieval,
) -> Result<jsonschema::Validator, String> {
    // Originals retain their retrieval bases; the tiny wrapper selects a named
    // fragment without cloning it and accidentally changing relative references.
    let mut builder = jsonschema::Registry::new().retriever(deny.clone());
    for (name, value) in catalog {
        builder = builder
            .add(name.as_str(), value)
            .map_err(|e| e.to_string())?;
    }
    let registry = builder.prepare().map_err(|e| e.to_string())?;
    jsonschema::options()
        .with_registry(&registry)
        .with_retriever(deny)
        .with_draft(jsonschema::Draft::Draft202012)
        .with_pattern_options(
            jsonschema::PatternOptions::fancy_regex()
                .backtrack_limit(20_000)
                .size_limit(1_048_576)
                .dfa_size_limit(1_048_576),
        )
        .build(&serde_json::json!({"$ref": uri}))
        .map_err(|e| e.to_string())
}

impl StructuralValidator {
    pub fn new() -> Result<Self, String> {
        let catalog = catalog()?;
        crate::schema_guard::check_catalog(&catalog)?;
        let mut validators = BTreeMap::new();
        for contract in [
            Contract::Quantity,
            Contract::SweRecord,
            Contract::PhysicalSystem,
            Contract::ObservationSwe,
            Contract::CommandSwe,
            Contract::JsonEncoding,
            Contract::TextEncoding,
            Contract::BinaryEncoding,
        ] {
            validators.insert(contract, compile(&catalog, &contract.uri())?);
        }
        Ok(Self { validators })
    }

    pub fn validate(&self, contract: Contract, input: &[u8]) -> Result<(), Failure> {
        let value = parse(input)?;
        if self.validators[&contract].is_valid(&value) {
            Ok(())
        } else {
            Err(Failure::Structure)
        }
    }

    /// Separately checked descriptor still has to match its enclosing format.
    pub fn validate_encoding(&self, format: Encoding, input: &[u8]) -> Result<(), Failure> {
        let value = parse(input)?;
        let (name, contract) = match format {
            Encoding::Json => ("JSONEncoding", Contract::JsonEncoding),
            Encoding::Text => ("TextEncoding", Contract::TextEncoding),
            Encoding::Binary => ("BinaryEncoding", Contract::BinaryEncoding),
        };
        if value.get("type").and_then(Value::as_str) != Some(name) {
            return Err(Failure::EncodingMismatch);
        }
        if self.validators[&contract].is_valid(&value) {
            Ok(())
        } else {
            Err(Failure::Structure)
        }
    }
}

#[cfg(test)]
mod tests;
