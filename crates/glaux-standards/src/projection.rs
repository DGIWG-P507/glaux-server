//! Fixed operation/direction projections; not an endpoint or admission decision.
//!
//! Original schemas are immutable. A separately compiled request catalog changes
//! only named required arrays. Ownership is explicit, not inferred recursively
//! from annotations. See docs/direction-validation.md for source attribution.

use crate::validation::{self, PIN};
use glaux_domain::identity::{LocalId, Uid};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Resource {
    SystemGeoJson,
    SystemSensorMl,
    DataStream,
    ControlStream,
    Observation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Projection {
    CreateRequest,
    ReplaceRequest,
    /// Complete candidate AFTER Merge Patch; separately check patch_members first.
    MergedPatch,
    Response,
}

/// Safe fixed paths/codes only; no submitted value or schema details in errors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProjectionError {
    Input(validation::Failure),
    Missing(&'static str),
    WriteOnly(&'static str),
    Protected(&'static str),
    UnmappedMember,
    Context,
    UnsupportedProjection,
    Structure,
}

/// Trusted endpoint/state context, never fields selected by the request.
/// System means the expected canonical parent-link href, NOT the System's UID.
/// Resolving alternate links to that parent is the later endpoint's job.
pub enum Parent<'a> {
    System(&'a Uid),
    Datastream(&'a LocalId),
}

#[derive(Default)]
pub struct RequestContext<'a> {
    /// Required on System replacement/complete PATCH candidates.
    pub existing_uid: Option<&'a Uid>,
    /// Required for stream/observation requests, even when the body omits it.
    pub parent: Option<Parent<'a>>,
    /// Supplied by the owning state check only when schema modification is barred.
    /// It is the retained schema, not a response field or a client permission.
    /// A complete PATCH candidate must include the retained write-only schema;
    /// merging into a response projection would incorrectly lose that content.
    pub locked_schema: Option<&'a [u8]>,
}

const RESOURCES: [Resource; 5] = [
    Resource::SystemGeoJson,
    Resource::SystemSensorMl,
    Resource::DataStream,
    Resource::ControlStream,
    Resource::Observation,
];
const DATA_GENERATED: &[&str] = &[
    "id",
    "formats",
    "system@link",
    "observedProperties",
    "phenomenonTime",
    "resultTime",
    "resultType",
    "live",
];
const CONTROL_GENERATED: &[&str] = &[
    "id",
    "formats",
    "system@link",
    "controlledProperties",
    "issueTime",
    "executionTime",
    "live",
];
const OBSERVATION_MEMBERS: &[&str] = &[
    "id",
    "datastream@id",
    "samplingFeature@id",
    "procedure@link",
    "phenomenonTime",
    "resultTime",
    "parameters",
    "result",
    "result@link",
];

impl Resource {
    fn uri(self) -> String {
        let path = match self {
            Self::SystemGeoJson => "api/part1/openapi/schemas/geojson/system.json",
            Self::SystemSensorMl => "api/part1/openapi/schemas/sensorml/system.json",
            Self::DataStream => "api/part2/openapi/schemas/json/dataStream.json",
            Self::ControlStream => "api/part2/openapi/schemas/json/controlStream.json",
            Self::Observation => "api/part2/openapi/schemas/json/observation.json",
        };
        format!("{PIN}{path}")
    }
    fn stream(self) -> bool {
        matches!(self, Self::DataStream | Self::ControlStream)
    }
    fn generated(self) -> &'static [&'static str] {
        match self {
            Self::SystemGeoJson | Self::SystemSensorMl => &["id", "links"],
            Self::DataStream => DATA_GENERATED,
            Self::ControlStream => CONTROL_GENERATED,
            Self::Observation => &["id", "datastream@id"],
        }
    }
    fn uid_path(self) -> Option<&'static str> {
        match self {
            Self::SystemGeoJson => Some("/properties/uid"),
            Self::SystemSensorMl => Some("/uniqueId"),
            _ => None,
        }
    }
}

/// Fail startup if an adaptation no longer matches its pinned source.
fn replace_required(
    catalog: &mut BTreeMap<String, Value>,
    path: &str,
    expected: &[&str],
    projected: &[&str],
) -> Result<(), String> {
    let schema = catalog
        .get_mut(&format!("{PIN}api/part2/openapi/schemas/json/{path}"))
        .ok_or("projection source absent")?;
    if schema.get("required") != Some(&json!(expected)) {
        return Err(format!("projection source required-array changed: {path}"));
    }
    schema["required"] = json!(projected);
    Ok(())
}

fn request_catalog(originals: &BTreeMap<String, Value>) -> Result<BTreeMap<String, Value>, String> {
    let mut result = originals.clone();
    replace_required(
        &mut result,
        "baseStream.json",
        &["id", "name", "formats"],
        &["name"],
    )?;
    replace_required(
        &mut result,
        "dataStream.json",
        &[
            "name",
            "system@link",
            "observedProperties",
            "phenomenonTime",
            "resultTime",
            "resultType",
            "live",
        ],
        &["name"],
    )?;
    replace_required(
        &mut result,
        "controlStream.json",
        &[
            "name",
            "system@link",
            "controlledProperties",
            "issueTime",
            "executionTime",
            "live",
            "async",
        ],
        &["name", "async"],
    )?;
    replace_required(
        &mut result,
        "observation.json",
        &["id", "datastream@id", "resultTime"],
        &["resultTime"],
    )?;
    // Property constraints, nested contracts, reference bases and every other
    // document remain intact. Catalog identity is internal; no edited schema is
    // exported under an upstream URL or mislabeled an original-source result.
    crate::schema_guard::check_catalog(&result)?;
    Ok(result)
}

pub struct ProjectionValidator {
    original: BTreeMap<Resource, jsonschema::Validator>,
    request: BTreeMap<Resource, jsonschema::Validator>,
}

impl ProjectionValidator {
    pub fn new() -> Result<Self, String> {
        let originals = validation::catalog()?;
        crate::schema_guard::check_catalog(&originals)?;
        let requests = request_catalog(&originals)?;
        let mut original = BTreeMap::new();
        let mut request = BTreeMap::new();
        for resource in RESOURCES {
            original.insert(resource, validation::compile(&originals, &resource.uri())?);
            request.insert(resource, validation::compile(&requests, &resource.uri())?);
        }
        Ok(Self { original, request })
    }

    /// Source-artifact diagnostic only, not direction-aware admission.
    pub fn validate_original(
        &self,
        resource: Resource,
        input: &[u8],
    ) -> Result<(), ProjectionError> {
        let value = validation::parse(input).map_err(ProjectionError::Input)?;
        if self.original[&resource].is_valid(&value) {
            Ok(())
        } else {
            Err(ProjectionError::Structure)
        }
    }

    /// Structural direction check only. Use request() for writable extraction
    /// with trusted UID/parent/locked-contract checks. Full semantics are later.
    pub fn validate(
        &self,
        resource: Resource,
        projection: Projection,
        input: &[u8],
    ) -> Result<(), ProjectionError> {
        let value = Self::direction_input(projection, input)?;
        self.validate_value(resource, projection, &value)
    }

    fn direction_input(projection: Projection, input: &[u8]) -> Result<Value, ProjectionError> {
        let mut value = validation::parse(input).map_err(ProjectionError::Input)?;
        // The selected transaction source SHALL ignore the submitted resource
        // identifier on PUT/PATCH, not only a well-shaped identifier. Never
        // erase nested IDs; never bypass raw/duplicate/budget parsing.
        if matches!(
            projection,
            Projection::ReplaceRequest | Projection::MergedPatch
        ) && let Some(members) = value.as_object_mut()
        {
            members.remove("id");
        }
        Ok(value)
    }

    fn validate_value(
        &self,
        resource: Resource,
        projection: Projection,
        value: &Value,
    ) -> Result<(), ProjectionError> {
        if !value.is_object() {
            return Err(ProjectionError::Structure);
        }
        if projection == Projection::MergedPatch && resource == Resource::SystemGeoJson {
            return Err(ProjectionError::UnsupportedProjection);
        }
        if projection == Projection::Response {
            if resource == Resource::Observation
                && value.as_object().is_some_and(|members| {
                    members
                        .keys()
                        .any(|name| !OBSERVATION_MEMBERS.contains(&name.as_str()))
                })
            {
                return Err(ProjectionError::UnmappedMember);
            }
            if resource.stream() && value.get("schema").is_some() {
                return Err(ProjectionError::WriteOnly("/schema"));
            }
            if matches!(resource, Resource::SystemGeoJson | Resource::SystemSensorMl) {
                for field in ["id", "links"] {
                    if value.get(field).is_none() {
                        return Err(ProjectionError::Missing(if field == "id" {
                            "/id"
                        } else {
                            "/links"
                        }));
                    }
                }
            }
            if !self.original[&resource].is_valid(value) {
                return Err(ProjectionError::Structure);
            }
        } else {
            if projection == Projection::CreateRequest
                && resource.stream()
                && value.get("schema").is_none()
            {
                return Err(ProjectionError::Missing("/schema"));
            }
            if !self.request[&resource].is_valid(value) {
                return Err(ProjectionError::Structure);
            }
        }
        Ok(())
    }

    /// Return only writable input, without inventing generated data/defaults.
    /// Does not apply PUT/PATCH, resolve links, compile SWE or authorize a write.
    pub fn request(
        &self,
        resource: Resource,
        projection: Projection,
        input: &[u8],
        context: RequestContext<'_>,
    ) -> Result<Value, ProjectionError> {
        if projection == Projection::Response {
            return Err(ProjectionError::UnsupportedProjection);
        }
        let mut value = Self::direction_input(projection, input)?;
        self.validate_value(resource, projection, &value)?;
        if let Some(path) = resource.uid_path() {
            let uid = value
                .pointer(path)
                .and_then(Value::as_str)
                .ok_or(ProjectionError::Missing(path))?
                .parse::<Uid>()
                .map_err(|_| ProjectionError::Structure)?;
            if projection != Projection::CreateRequest && context.existing_uid.is_none() {
                return Err(ProjectionError::Context);
            }
            if context
                .existing_uid
                .is_some_and(|expected| expected != &uid)
            {
                return Err(ProjectionError::Protected(path));
            }
        }
        match (resource, context.parent) {
            (Resource::DataStream | Resource::ControlStream, Some(Parent::System(parent))) => {
                if let Some(supplied) = value.get("system@link")
                    && supplied.get("href").and_then(Value::as_str) != Some(parent.as_str())
                {
                    return Err(ProjectionError::Protected("/system@link"));
                }
            }
            (Resource::Observation, Some(Parent::Datastream(parent))) => {
                if let Some(supplied) = value.get("datastream@id")
                    && supplied.as_str() != Some(parent.to_string().as_str())
                {
                    return Err(ProjectionError::Protected("/datastream@id"));
                }
            }
            (Resource::SystemGeoJson | Resource::SystemSensorMl, None) => {}
            _ => return Err(ProjectionError::Context),
        }
        if let Some(retained) = context.locked_schema {
            if !resource.stream() {
                return Err(ProjectionError::Context);
            }
            let retained = validation::parse(retained).map_err(ProjectionError::Input)?;
            if value
                .get("schema")
                .is_some_and(|submitted| submitted != &retained)
                || (projection == Projection::MergedPatch && value.get("schema").is_none())
            {
                return Err(ProjectionError::Protected("/schema"));
            }
        }
        let members = value.as_object_mut().ok_or(ProjectionError::Structure)?;
        for field in resource.generated() {
            members.remove(*field);
        }
        if resource == Resource::Observation {
            members.retain(|name, _| OBSERVATION_MEMBERS.contains(&name.as_str()));
        }
        Ok(value)
    }

    /// Inspect the actual partial PATCH BEFORE merge/restoration can hide intent.
    /// Return its writable members; caller later applies RFC 7396 and validates
    /// the complete MergedPatch candidate with trusted context and state rules.
    pub fn patch_members(
        &self,
        resource: Resource,
        input: &[u8],
    ) -> Result<Value, ProjectionError> {
        let mut value = validation::parse(input).map_err(ProjectionError::Input)?;
        if resource == Resource::SystemGeoJson {
            return Err(ProjectionError::UnsupportedProjection);
        }
        let members = value.as_object_mut().ok_or(ProjectionError::Structure)?;
        // The selected transaction draft ignores the outer identifier, including
        // a null patch value. Nested component IDs remain client-authored content.
        members.remove("id");
        // An unchanged UID echo is not a change. request(MergedPatch) compares
        // the complete candidate to trusted existing_uid BEFORE restoration.
        for field in resource.generated().iter().filter(|field| **field != "id") {
            if members.contains_key(*field) {
                let path = match *field {
                    "links" => "/links",
                    "formats" => "/formats",
                    "system@link" => "/system@link",
                    "datastream@id" => "/datastream@id",
                    "observedProperties" => "/observedProperties",
                    "controlledProperties" => "/controlledProperties",
                    "phenomenonTime" => "/phenomenonTime",
                    "resultTime" => "/resultTime",
                    "resultType" => "/resultType",
                    "issueTime" => "/issueTime",
                    "executionTime" => "/executionTime",
                    "live" => "/live",
                    _ => unreachable!("fixed generated field"),
                };
                return Err(ProjectionError::Protected(path));
            }
        }
        if resource == Resource::Observation {
            members.retain(|name, _| OBSERVATION_MEMBERS.contains(&name.as_str()));
        }
        Ok(value)
    }
}

#[cfg(test)]
mod policy_tests;
#[cfg(test)]
mod tests;
