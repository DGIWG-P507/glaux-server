//! Shared SensorML descriptive content, not a complete resource-family mapping.
//!
//! Sources: the unchanged SensorML JSON schemas at the corpus's CSAPI v1.0.0
//! pin, especially DescribedObject, commonDefs/Term, ResponsibleParty,
//! ContactLink, Document and common/link/timePeriod. Unknown members are retained
//! without granting identity, relationship, execution or access-policy meaning.
//! DerivedProperty does not inherit DescribedObject and is not routed here.
use glaux_domain::temporal::{ExactInstant, TimeError};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::validation::{self, Contract, StructuralValidator};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DescriptionError {
    Syntax(validation::Failure),
    Structure,
    MediaType,
    Time(TimeError),
    ReversedValidity,
    ReservedExtension,
    ExtensionNamespace,
    ExtensionCollision,
}

/// Immutable typed common meaning and the independent original artifact.
/// Successful construction validates only DescribedObject and this common
/// mapping. Family-specific members in `remaining_members` are not validated
/// against their family or interpreted as relationships by this API.
pub struct CommonDescription {
    source: Vec<u8>,
    media_type: String,
    sha256: [u8; 32],
    type_name: String,
    id: Option<String>,
    unique_id: String,
    label: String,
    description: Option<String>,
    lang: Option<String>,
    keywords: Option<Vec<String>>,
    identifiers: Option<Vec<Term>>,
    classifiers: Option<Vec<Term>>,
    contacts: Option<Vec<Contact>>,
    documents: Option<Vec<Document>>,
    valid_time: Option<ValidityPeriod>,
    remaining: Map<String, Value>,
}

impl CommonDescription {
    pub fn compile(
        validator: &StructuralValidator,
        input: &[u8],
        media_type: &str,
    ) -> Result<Self, DescriptionError> {
        if media_type.is_empty()
            || media_type.len() > 256
            || !media_type.bytes().all(|byte| (0x20..=0x7e).contains(&byte))
        {
            return Err(DescriptionError::MediaType);
        }
        let value = validation::parse(input).map_err(DescriptionError::Syntax)?;
        validator
            .validate(Contract::DescribedObject, input)
            .map_err(DescriptionError::Syntax)?;
        let object = object(&value)?;
        let valid_time = object
            .get("validTime")
            .map(ValidityPeriod::parse)
            .transpose()?;
        let keywords = object
            .get("keywords")
            .map(|value| {
                value
                    .as_array()
                    .ok_or(DescriptionError::Structure)?
                    .iter()
                    .map(|value| {
                        value
                            .as_str()
                            .map(str::to_owned)
                            .ok_or(DescriptionError::Structure)
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?;
        let mut remaining = object.clone();
        for key in COMMON_FIELDS {
            remaining.remove(*key);
        }
        Ok(Self {
            source: input.to_vec(),
            media_type: media_type.to_owned(),
            sha256: Sha256::digest(input).into(),
            type_name: required(object, "type")?,
            id: optional(object, "id")?,
            unique_id: required(object, "uniqueId")?,
            label: required(object, "label")?,
            description: optional(object, "description")?,
            lang: optional(object, "lang")?,
            keywords,
            identifiers: list(object, "identifiers", Term::parse)?,
            classifiers: list(object, "classifiers", Term::parse)?,
            contacts: list(object, "contacts", Contact::parse)?,
            documents: list(object, "documents", Document::parse)?,
            valid_time,
            remaining,
        })
    }

    pub fn source_bytes(&self) -> &[u8] {
        &self.source
    }
    pub fn source_media_type(&self) -> &str {
        &self.media_type
    }
    pub fn source_sha256(&self) -> &[u8; 32] {
        &self.sha256
    }
    pub fn type_name(&self) -> &str {
        &self.type_name
    }
    pub fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }
    pub fn unique_id(&self) -> &str {
        &self.unique_id
    }
    pub fn label(&self) -> &str {
        &self.label
    }
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }
    pub fn lang(&self) -> Option<&str> {
        self.lang.as_deref()
    }
    pub fn keywords(&self) -> Option<&[String]> {
        self.keywords.as_deref()
    }
    pub fn identifiers(&self) -> Option<&[Term]> {
        self.identifiers.as_deref()
    }
    pub fn classifiers(&self) -> Option<&[Term]> {
        self.classifiers.as_deref()
    }
    pub fn contacts(&self) -> Option<&[Contact]> {
        self.contacts.as_deref()
    }
    pub fn documents(&self) -> Option<&[Document]> {
        self.documents.as_deref()
    }
    pub fn valid_time(&self) -> Option<&ValidityPeriod> {
        self.valid_time.as_ref()
    }
    pub fn remaining_members(&self) -> &Map<String, Value> {
        &self.remaining
    }

    /// Reconstruct typed mapped fields, preserving list order, optional-member
    /// presence and uninterpreted remainder. This is generated JSON, not the
    /// source artifact or an authorized HTTP response.
    pub fn generated_json(&self) -> Value {
        let mut out = self.remaining.clone();
        out.insert("type".into(), Value::String(self.type_name.clone()));
        put_string(&mut out, "id", self.id.as_deref());
        out.insert("uniqueId".into(), Value::String(self.unique_id.clone()));
        out.insert("label".into(), Value::String(self.label.clone()));
        put_string(&mut out, "description", self.description.as_deref());
        put_string(&mut out, "lang", self.lang.as_deref());
        if let Some(values) = &self.keywords {
            out.insert(
                "keywords".into(),
                Value::Array(values.iter().cloned().map(Value::String).collect()),
            );
        }
        put_list(
            &mut out,
            "identifiers",
            self.identifiers.as_deref(),
            Term::json,
        );
        put_list(
            &mut out,
            "classifiers",
            self.classifiers.as_deref(),
            Term::json,
        );
        put_list(
            &mut out,
            "contacts",
            self.contacts.as_deref(),
            Contact::json,
        );
        put_list(
            &mut out,
            "documents",
            self.documents.as_deref(),
            Document::json,
        );
        if let Some(period) = &self.valid_time {
            out.insert("validTime".into(), period.json());
        }
        Value::Object(out)
    }

    /// Explicit Glaux helper for adding qualified extension members to a
    /// generated common description. It is not a generic input-rejection rule
    /// or an HTTP extension-advertisement policy. A `prefix:local` key uses a
    /// separate namespace; known SensorML/SWE prefixes are not extension space.
    /// Existing members cannot be overwritten and extension bodies stay nested.
    pub fn with_extensions(
        &self,
        extensions: &Map<String, Value>,
    ) -> Result<Value, DescriptionError> {
        bound_extensions(extensions)?;
        let Value::Object(mut out) = self.generated_json() else {
            unreachable!()
        };
        for (name, value) in extensions {
            if COMMON_FIELDS.contains(&name.as_str()) || RESERVED_FIELDS.contains(&name.as_str()) {
                return Err(DescriptionError::ReservedExtension);
            }
            if out.contains_key(name) {
                return Err(DescriptionError::ExtensionCollision);
            }
            let Some((prefix, local)) = name.split_once(':') else {
                return Err(DescriptionError::ExtensionNamespace);
            };
            if prefix.is_empty()
                || local.is_empty()
                || !prefix.as_bytes()[0].is_ascii_alphabetic()
                || !prefix
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"+.-".contains(&byte))
                || ["sml", "sensorml", "swe", "gml", "geojson", "ogc"]
                    .iter()
                    .any(|reserved| prefix.eq_ignore_ascii_case(reserved))
            {
                return Err(DescriptionError::ExtensionNamespace);
            }
            out.insert(name.clone(), value.clone());
        }
        let generated = Value::Object(out);
        let bytes = serde_json::to_vec(&generated).map_err(|_| DescriptionError::Structure)?;
        validation::parse(&bytes).map_err(DescriptionError::Syntax)?;
        Ok(generated)
    }
}

const COMMON_FIELDS: &[&str] = &[
    "type",
    "id",
    "uniqueId",
    "label",
    "description",
    "lang",
    "keywords",
    "identifiers",
    "classifiers",
    "contacts",
    "documents",
    "validTime",
];

// Do not allow an extension-insertion API to create unvalidated ordinary
// SensorML/CSAPI identity, content or relationship members. Source members with
// these names remain intact in the explicitly uninterpreted remainder instead.
const RESERVED_FIELDS: &[&str] = &[
    "securityConstraints",
    "legalConstraints",
    "characteristics",
    "capabilities",
    "history",
    "definition",
    "typeOf",
    "configuration",
    "featuresOfInterest",
    "inputs",
    "outputs",
    "parameters",
    "modes",
    "method",
    "components",
    "connections",
    "attachedTo",
    "position",
    "localReferenceFrames",
    "localTimeFrames",
    "location",
    "platform",
    "deployedSystems",
    "baseProperty",
    "objectType",
    "statistic",
    "qualifiers",
    "links",
    "geometry",
    "properties",
    "featureType",
    "systemType",
    "systemKind",
    "systemKind@link",
    "parentSystem",
    "parentSystem@link",
    "system",
    "system@link",
    "procedure",
    "procedures",
    "deployment",
    "deployments",
    "sampledFeature",
    "sampledFeature@link",
    "samplingFeature",
    "samplingFeatures",
];

#[derive(Clone, Debug, PartialEq)]
pub struct Term {
    pub definition: Option<String>,
    pub label: String,
    pub code_space: Option<String>,
    pub value: String,
}

impl Term {
    fn parse(value: &Value) -> Result<Self, DescriptionError> {
        let object = object(value)?;
        Ok(Self {
            definition: optional(object, "definition")?,
            label: required(object, "label")?,
            code_space: optional(object, "codeSpace")?,
            value: required(object, "value")?,
        })
    }
    fn json(&self) -> Value {
        let mut out = Map::new();
        put_string(&mut out, "definition", self.definition.as_deref());
        out.insert("label".into(), Value::String(self.label.clone()));
        put_string(&mut out, "codeSpace", self.code_space.as_deref());
        out.insert("value".into(), Value::String(self.value.clone()));
        Value::Object(out)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Contact {
    Party(ResponsibleParty),
    Link(ContactLink),
}
impl Contact {
    fn parse(value: &Value) -> Result<Self, DescriptionError> {
        if object(value)?.contains_key("link") {
            ContactLink::parse(value).map(Self::Link)
        } else {
            ResponsibleParty::parse(value).map(Self::Party)
        }
    }
    fn json(&self) -> Value {
        match self {
            Self::Party(value) => value.json(),
            Self::Link(value) => value.json(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResponsibleParty {
    pub individual_name: Option<String>,
    pub organisation_name: Option<String>,
    pub position_name: Option<String>,
    pub role: String,
    pub contact_info: Option<ContactInfo>,
}
impl ResponsibleParty {
    fn parse(value: &Value) -> Result<Self, DescriptionError> {
        let object = object(value)?;
        Ok(Self {
            individual_name: optional(object, "individualName")?,
            organisation_name: optional(object, "organisationName")?,
            position_name: optional(object, "positionName")?,
            role: required(object, "role")?,
            contact_info: object
                .get("contactInfo")
                .map(ContactInfo::parse)
                .transpose()?,
        })
    }
    fn json(&self) -> Value {
        let mut out = Map::new();
        put_string(&mut out, "individualName", self.individual_name.as_deref());
        put_string(
            &mut out,
            "organisationName",
            self.organisation_name.as_deref(),
        );
        put_string(&mut out, "positionName", self.position_name.as_deref());
        out.insert("role".into(), Value::String(self.role.clone()));
        if let Some(info) = &self.contact_info {
            out.insert("contactInfo".into(), info.json());
        }
        Value::Object(out)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ContactInfo {
    pub phone: Option<Phone>,
    pub address: Option<Address>,
    pub website: Option<String>,
    pub hours_of_service: Option<String>,
    pub contact_instructions: Option<String>,
}
impl ContactInfo {
    fn parse(value: &Value) -> Result<Self, DescriptionError> {
        let object = object(value)?;
        Ok(Self {
            phone: object.get("phone").map(Phone::parse).transpose()?,
            address: object.get("address").map(Address::parse).transpose()?,
            website: optional(object, "website")?,
            hours_of_service: optional(object, "hoursOfService")?,
            contact_instructions: optional(object, "contactInstructions")?,
        })
    }
    fn json(&self) -> Value {
        let mut out = Map::new();
        if let Some(value) = &self.phone {
            out.insert("phone".into(), value.json());
        }
        if let Some(value) = &self.address {
            out.insert("address".into(), value.json());
        }
        put_string(&mut out, "website", self.website.as_deref());
        put_string(&mut out, "hoursOfService", self.hours_of_service.as_deref());
        put_string(
            &mut out,
            "contactInstructions",
            self.contact_instructions.as_deref(),
        );
        Value::Object(out)
    }
}

// These closed, all-optional string objects follow ResponsibleParty.json.
// The macro avoids subtly different absent/null treatment across their fields.
macro_rules! string_object {
    ($name:ident { $($field:ident => $wire:literal),* $(,)? }) => {
        #[derive(Clone, Debug, PartialEq)]
        pub struct $name { $(pub $field: Option<String>),* }
        impl $name {
            fn parse(value: &Value) -> Result<Self, DescriptionError> {
                let object = object(value)?;
                Ok(Self { $($field: optional(object, $wire)?),* })
            }
            fn json(&self) -> Value {
                let mut out = Map::new();
                $(put_string(&mut out, $wire, self.$field.as_deref());)*
                Value::Object(out)
            }
        }
    };
}
string_object!(Phone { voice => "voice", facsimile => "facsimile" });
string_object!(Address {
    delivery_point => "deliveryPoint", city => "city", administrative_area => "administrativeArea",
    postal_code => "postalCode", country => "country", electronic_mail_address => "electronicMailAddress",
});

#[derive(Clone, Debug, PartialEq)]
pub struct ContactLink {
    pub role: Option<String>,
    pub name: String,
    pub link: Link,
}
impl ContactLink {
    fn parse(value: &Value) -> Result<Self, DescriptionError> {
        let object = object(value)?;
        Ok(Self {
            role: optional(object, "role")?,
            name: required(object, "name")?,
            link: Link::parse(object.get("link").ok_or(DescriptionError::Structure)?)?,
        })
    }
    fn json(&self) -> Value {
        let mut out = Map::new();
        put_string(&mut out, "role", self.role.as_deref());
        out.insert("name".into(), Value::String(self.name.clone()));
        out.insert("link".into(), self.link.json());
        Value::Object(out)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    pub role: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub link: Link,
}
impl Document {
    fn parse(value: &Value) -> Result<Self, DescriptionError> {
        let object = object(value)?;
        Ok(Self {
            role: optional(object, "role")?,
            name: required(object, "name")?,
            description: optional(object, "description")?,
            link: Link::parse(object.get("link").ok_or(DescriptionError::Structure)?)?,
        })
    }
    fn json(&self) -> Value {
        let mut out = Map::new();
        put_string(&mut out, "role", self.role.as_deref());
        out.insert("name".into(), Value::String(self.name.clone()));
        put_string(&mut out, "description", self.description.as_deref());
        out.insert("link".into(), self.link.json());
        Value::Object(out)
    }
}

/// The open common Link shape. Unknown members are preserved only as metadata,
/// not resolved, flattened, or used as authenticated relationships.
#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    pub href: String,
    pub rel: Option<String>,
    pub media_type: Option<String>,
    pub hreflang: Option<String>,
    pub title: Option<String>,
    pub uid: Option<String>,
    pub resource_type: Option<String>,
    pub interface: Option<String>,
    pub remaining: Map<String, Value>,
}
impl Link {
    fn parse(value: &Value) -> Result<Self, DescriptionError> {
        let object = object(value)?;
        let mut remaining = object.clone();
        for key in [
            "href", "rel", "type", "hreflang", "title", "uid", "rt", "if",
        ] {
            remaining.remove(key);
        }
        Ok(Self {
            href: required(object, "href")?,
            rel: optional(object, "rel")?,
            media_type: optional(object, "type")?,
            hreflang: optional(object, "hreflang")?,
            title: optional(object, "title")?,
            uid: optional(object, "uid")?,
            resource_type: optional(object, "rt")?,
            interface: optional(object, "if")?,
            remaining,
        })
    }
    fn json(&self) -> Value {
        let mut out = self.remaining.clone();
        out.insert("href".into(), Value::String(self.href.clone()));
        put_string(&mut out, "rel", self.rel.as_deref());
        put_string(&mut out, "type", self.media_type.as_deref());
        put_string(&mut out, "hreflang", self.hreflang.as_deref());
        put_string(&mut out, "title", self.title.as_deref());
        put_string(&mut out, "uid", self.uid.as_deref());
        put_string(&mut out, "rt", self.resource_type.as_deref());
        put_string(&mut out, "if", self.interface.as_deref());
        Value::Object(out)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ValidityBound {
    Instant(ExactInstant),
    Now,
}
impl ValidityBound {
    fn parse(value: &Value) -> Result<Self, DescriptionError> {
        let text = value.as_str().ok_or(DescriptionError::Structure)?;
        if text == "now" {
            Ok(Self::Now)
        } else {
            ExactInstant::parse_rfc3339(text)
                .map(Self::Instant)
                .map_err(DescriptionError::Time)
        }
    }
    fn json(&self) -> Value {
        Value::String(match self {
            Self::Instant(value) => value.source_lexeme().to_owned(),
            Self::Now => "now".into(),
        })
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct ValidityPeriod {
    pub start: ValidityBound,
    pub end: ValidityBound,
}
impl ValidityPeriod {
    fn parse(value: &Value) -> Result<Self, DescriptionError> {
        let values = value.as_array().ok_or(DescriptionError::Structure)?;
        if values.len() != 2 {
            return Err(DescriptionError::Structure);
        }
        let start = ValidityBound::parse(&values[0])?;
        let end = ValidityBound::parse(&values[1])?;
        if let (ValidityBound::Instant(start), ValidityBound::Instant(end)) = (&start, &end)
            && start > end
        {
            return Err(DescriptionError::ReversedValidity);
        }
        Ok(Self { start, end })
    }
    fn json(&self) -> Value {
        Value::Array(vec![self.start.json(), self.end.json()])
    }
}

fn object(value: &Value) -> Result<&Map<String, Value>, DescriptionError> {
    value.as_object().ok_or(DescriptionError::Structure)
}
fn required(object: &Map<String, Value>, name: &str) -> Result<String, DescriptionError> {
    object
        .get(name)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or(DescriptionError::Structure)
}
fn optional(object: &Map<String, Value>, name: &str) -> Result<Option<String>, DescriptionError> {
    object
        .get(name)
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or(DescriptionError::Structure)
        })
        .transpose()
}
fn list<T>(
    object: &Map<String, Value>,
    name: &str,
    parse: fn(&Value) -> Result<T, DescriptionError>,
) -> Result<Option<Vec<T>>, DescriptionError> {
    object
        .get(name)
        .map(|value| {
            value
                .as_array()
                .ok_or(DescriptionError::Structure)?
                .iter()
                .map(parse)
                .collect()
        })
        .transpose()
}
fn put_string(out: &mut Map<String, Value>, name: &str, value: Option<&str>) {
    if let Some(value) = value {
        out.insert(name.into(), Value::String(value.to_owned()));
    }
}
fn put_list<T>(
    out: &mut Map<String, Value>,
    name: &str,
    values: Option<&[T]>,
    emit: fn(&T) -> Value,
) {
    if let Some(values) = values {
        out.insert(name.into(), Value::Array(values.iter().map(emit).collect()));
    }
}

/// A caller may construct a Value without using our parser. Bound it iteratively
/// before cloning or recursively serializing it. The raw-size lower bound limits
/// temporary work; exact escaped JSON size is checked by parse after generation.
fn bound_extensions(extensions: &Map<String, Value>) -> Result<(), DescriptionError> {
    use validation::{Failure, MAX_BYTES, MAX_DEPTH, MAX_MEMBERS, MAX_NODES, MAX_STRING_BYTES};
    let fail = DescriptionError::Syntax;
    if extensions.len() > MAX_MEMBERS {
        return Err(fail(Failure::Members));
    }
    let mut bytes = 0_usize;
    let mut charge = |len: usize| -> Result<(), DescriptionError> {
        bytes = bytes.saturating_add(len);
        if bytes > MAX_BYTES {
            Err(fail(Failure::Size))
        } else {
            Ok(())
        }
    };
    for name in extensions.keys() {
        if name.len() > MAX_STRING_BYTES {
            return Err(fail(Failure::String));
        }
        charge(name.len())?;
    }
    let mut pending: Vec<_> = extensions.values().map(|value| (value, 2)).collect();
    let mut nodes = 1;
    while let Some((value, depth)) = pending.pop() {
        nodes += 1;
        if nodes > MAX_NODES {
            return Err(fail(Failure::Nodes));
        }
        charge(1)?;
        match value {
            Value::Object(object) => {
                if depth > MAX_DEPTH {
                    return Err(fail(Failure::Depth));
                }
                if object.len() > MAX_MEMBERS {
                    return Err(fail(Failure::Members));
                }
                for (name, value) in object {
                    if name.len() > MAX_STRING_BYTES {
                        return Err(fail(Failure::String));
                    }
                    charge(name.len())?;
                    pending.push((value, depth + 1));
                }
            }
            Value::Array(values) => {
                if depth > MAX_DEPTH {
                    return Err(fail(Failure::Depth));
                }
                if values.len() > MAX_MEMBERS {
                    return Err(fail(Failure::Members));
                }
                pending.extend(values.iter().map(|value| (value, depth + 1)));
            }
            Value::String(value) => {
                if value.len() > MAX_STRING_BYTES {
                    return Err(fail(Failure::String));
                }
                charge(value.len())?;
            }
            Value::Number(value) => charge(value.as_str().len())?,
            Value::Bool(_) | Value::Null => {}
        }
        if nodes + pending.len() > MAX_NODES {
            return Err(fail(Failure::Nodes));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
