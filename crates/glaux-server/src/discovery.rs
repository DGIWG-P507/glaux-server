//! Explicit discovery of installed routes, without completed-class claims.
use crate::configuration::Authentication;
use crate::http_boundary::{HttpBoundary, Problem, json_response, negotiate};
use axum::Router;
use axum::body::Bytes;
use axum::handler::Handler;
use axum::http::{HeaderMap, HeaderValue, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{MethodRouter, get, post};
use serde_json::{Map, Value, json};
use std::sync::Arc;

const DECLARED_CLASSES: &[&str] = &[]; // DISCOVERY_DECLARATION
const CONFORMANCE_REL: &str = "http://www.opengis.net/def/rel/ogc/1.0/conformance";
const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self' data:; font-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'none'";

#[derive(Clone, Copy)]
enum Family {
    Discovery,
    Documentation,
    Download,
    Health,
    Registration,
}

impl Family {
    fn name(self) -> &'static str {
        match self {
            Self::Discovery => "Discovery",
            Self::Documentation => "Documentation",
            Self::Download => "Offline downloads",
            Self::Health => "Operational health (Glaux extension)",
            Self::Registration => "Initial System registration and retrieval",
        }
    }
}

#[derive(Clone, Copy)]
enum Parameters {
    Accept,
    None,
}

#[derive(Clone, Copy)]
enum Methods {
    GetHead,
    Post,
}

/// Ordinary code metadata, also used when installing existing health handlers.
#[derive(Clone, Copy)]
pub struct RouteDefinition {
    path: &'static str,
    segments: &'static [&'static str],
    operation: &'static str,
    title: &'static str,
    family: Family,
    media: &'static str,
    methods: Methods,
    parameters: Parameters,
    conformance_dependencies: &'static [&'static str],
}

impl RouteDefinition {
    pub const fn path(self) -> &'static str {
        self.path
    }

    pub fn method<T, S>(self, handler: impl Handler<T, S>) -> MethodRouter<S>
    where
        T: 'static,
        S: Clone + Send + Sync + 'static,
    {
        match self.methods {
            Methods::GetHead => get(handler),
            Methods::Post => post(handler),
        }
    }

    fn link(self, boundary: &HttpBoundary, relation: &str) -> Result<Value, Problem> {
        Ok(json!({
            "href": boundary.link(self.segments, &[])?,
            "rel": relation,
            "type": self.media,
            "title": self.title
        }))
    }
}

const fn route(
    path: &'static str,
    segments: &'static [&'static str],
    operation: &'static str,
    title: &'static str,
    family: Family,
    media: &'static str,
) -> RouteDefinition {
    RouteDefinition {
        path,
        segments,
        operation,
        title,
        family,
        media,
        methods: Methods::GetHead,
        parameters: match family {
            Family::Health => Parameters::None,
            _ => Parameters::Accept,
        },
        conformance_dependencies: &[],
    }
}

const LANDING: RouteDefinition = route(
    "/",
    &[],
    "landing",
    "Landing page",
    Family::Discovery,
    "application/json",
);
const CONFORMANCE: RouteDefinition = route(
    "/conformance",
    &["conformance"],
    "conformance",
    "Conformance declaration",
    Family::Discovery,
    "application/json",
);
const API: RouteDefinition = route(
    "/api",
    &["api"],
    "api",
    "OpenAPI 3.1 description",
    Family::Documentation,
    "application/json",
);
const DOCS: RouteDefinition = route(
    "/docs",
    &["docs"],
    "docs",
    "Human-readable API documentation",
    Family::Documentation,
    "text/html;charset=utf-8",
);
const INIT: RouteDefinition = route(
    "/docs/init.js",
    &["docs", "init.js"],
    "documentationInitializer",
    "Local documentation initializer",
    Family::Documentation,
    "text/javascript;charset=utf-8",
);
const BUNDLE: RouteDefinition = route(
    "/docs/swagger-ui-bundle.js",
    &["docs", "swagger-ui-bundle.js"],
    "documentationBundle",
    "Pinned Swagger UI bundle",
    Family::Documentation,
    "text/javascript;charset=utf-8",
);
const CSS: RouteDefinition = route(
    "/docs/swagger-ui.css",
    &["docs", "swagger-ui.css"],
    "documentationStyle",
    "Pinned Swagger UI stylesheet",
    Family::Documentation,
    "text/css;charset=utf-8",
);
const LICENSE: RouteDefinition = route(
    "/docs/LICENSE",
    &["docs", "LICENSE"],
    "documentationLicense",
    "Swagger UI licence",
    Family::Download,
    "text/plain;charset=utf-8",
);
const NOTICE: RouteDefinition = route(
    "/docs/NOTICE",
    &["docs", "NOTICE"],
    "documentationNotice",
    "Swagger UI notice",
    Family::Download,
    "text/plain;charset=utf-8",
);
const THIRD_PARTY: RouteDefinition = route(
    "/docs/swagger-ui-bundle.js.LICENSE.txt",
    &["docs", "swagger-ui-bundle.js.LICENSE.txt"],
    "documentationThirdPartyNotices",
    "Swagger UI bundled notices",
    Family::Download,
    "text/plain;charset=utf-8",
);
const SCHEMA: RouteDefinition = route(
    "/schemas/discovery.json",
    &["schemas", "discovery.json"],
    "discoverySchema",
    "Original Glaux discovery schema",
    Family::Download,
    "application/schema+json",
);
const LANDING_EXAMPLE: RouteDefinition = route(
    "/examples/landing.json",
    &["examples", "landing.json"],
    "landingExample",
    "Current deployment landing example",
    Family::Download,
    "application/json",
);
const CONFORMANCE_EXAMPLE: RouteDefinition = route(
    "/examples/conformance.json",
    &["examples", "conformance.json"],
    "conformanceExample",
    "Current deployment conformance example",
    Family::Download,
    "application/json",
);
pub const HEALTH_LIVE: RouteDefinition = route(
    "/health/live",
    &["health", "live"],
    "liveness",
    "Operational liveness",
    Family::Health,
    "text/plain; charset=utf-8",
);
pub const HEALTH_READY: RouteDefinition = route(
    "/health/ready",
    &["health", "ready"],
    "readiness",
    "Operational storage readiness",
    Family::Health,
    "text/plain; charset=utf-8",
);

/// The same metadata selects the actual registration handler's path and method.
pub const SYSTEM_CREATE: RouteDefinition = RouteDefinition {
    methods: Methods::Post,
    parameters: Parameters::None,
    ..route(
        "/systems",
        &["systems"],
        "createSystem",
        "Create a minimal System",
        Family::Registration,
        "application/geo+json",
    )
};

/// Canonical member read, installed with creation. The handler appends the
/// server-generated local ID to these segments; there is no collection GET.
pub const SYSTEM_READ: RouteDefinition = RouteDefinition {
    parameters: Parameters::None,
    ..route(
        "/systems/{id}",
        &["systems"],
        "getSystem",
        "Retrieve a minimal System",
        Family::Registration,
        "application/geo+json",
    )
};

const SYSTEM_TYPES: [&str; 10] = [
    "sosa:Sensor",
    "sosa:Actuator",
    "sosa:Sampler",
    "sosa:Platform",
    "sosa:System",
    "http://www.w3.org/ns/sosa/Sensor",
    "http://www.w3.org/ns/sosa/Actuator",
    "http://www.w3.org/ns/sosa/Sampler",
    "http://www.w3.org/ns/sosa/Platform",
    "http://www.w3.org/ns/sosa/System",
];
const EXAMPLE_SYSTEM_ID: &str = "0190f5c2-7b5a-7cc3-98c4-dc0c0c220001";

const ROUTES: [RouteDefinition; 15] = [
    LANDING,
    CONFORMANCE,
    API,
    DOCS,
    INIT,
    BUNDLE,
    CSS,
    LICENSE,
    NOTICE,
    THIRD_PARTY,
    SCHEMA,
    LANDING_EXAMPLE,
    CONFORMANCE_EXAMPLE,
    HEALTH_LIVE,
    HEALTH_READY,
];

fn landing(boundary: &HttpBoundary) -> Result<Value, Problem> {
    Ok(json!({
        "title": "Glaux Server",
        "description": "Initial discovery and documentation. No CSAPI resource family or standards conformance class is yet advertised.",
        "links": [
            LANDING.link(boundary, "self")?,
            CONFORMANCE.link(boundary, CONFORMANCE_REL)?,
            API.link(boundary, "service-desc")?,
            DOCS.link(boundary, "service-doc")?,
            SCHEMA.link(boundary, "describedby")?,
            LANDING_EXAMPLE.link(boundary, "related")?,
            CONFORMANCE_EXAMPLE.link(boundary, "related")?
        ]
    }))
}

fn conformance(boundary: &HttpBoundary) -> Result<Value, Problem> {
    Ok(json!({
        "conformsTo": DECLARED_CLASSES,
        "links": [CONFORMANCE.link(boundary, "self")?, LANDING.link(boundary, "up")?]
    }))
}

fn success(route: RouteDefinition, head: bool, description: &str) -> Value {
    let mut response = json!({
        "description": if head { format!("{description}. Headers for the GET representation ({}) without a response body.", route.media) } else { description.to_owned() },
        "headers": {
            "Cache-Control": {"schema": {"type": "string", "const": "no-store"}}
        }
    });
    if !head {
        let mut content = Map::new();
        let schema = match route.path {
            "/" | "/examples/landing.json" => json!({"$ref": "#/components/schemas/Landing"}),
            "/conformance" | "/examples/conformance.json" => {
                json!({"$ref": "#/components/schemas/Conformance"})
            }
            "/api" | "/schemas/discovery.json" => json!({"type": "object"}),
            _ => json!({"type": "string"}),
        };
        content.insert(route.media.to_owned(), json!({"schema": schema}));
        response["content"] = Value::Object(content);
    }
    response
}

fn openapi(
    boundary: &HttpBoundary,
    schema: &Value,
    system_creation: bool,
    authentication: Authentication,
) -> Result<Value, Problem> {
    let mut paths = Map::new();
    for route in ROUTES {
        let mut methods = Map::new();
        let method_names: &[&str] = match route.methods {
            Methods::GetHead => &["get", "head"],
            Methods::Post => &["post"],
        };
        for &method in method_names {
            let head = method == "head";
            let mut responses = Map::new();
            responses.insert(
                "200".to_owned(),
                success(route, head, "Successful response"),
            );
            if route.path == HEALTH_READY.path {
                responses.insert(
                    "503".to_owned(),
                    success(route, head, "Storage is not ready"),
                );
            }
            let mut error = json!({
                "description": "Safe bounded HTTP error using application/problem+json; HEAD omits the response body."
            });
            if !head {
                error["content"] = json!({"application/problem+json": {"schema": {"$ref": "#/components/schemas/Problem"}}});
            }
            responses.insert("default".to_owned(), error);
            let mut operation = json!({
                "operationId": format!("{}{method}", route.operation),
                "summary": route.title,
                "tags": [route.family.name()],
                "security": [],
                "responses": responses,
                "x-glaux-conformance-dependencies": route.conformance_dependencies
            });
            if matches!(route.parameters, Parameters::Accept) {
                // OAS 3.1 ignores an Accept Header Parameter Object. Describe
                // HTTP negotiation without inventing a query parameter.
                operation["description"] = json!(format!(
                    "Standard HTTP Accept negotiation offers {}. No query parameters are defined.",
                    route.media
                ));
            }
            methods.insert(method.to_owned(), operation);
        }
        paths.insert(route.path.to_owned(), Value::Object(methods));
    }
    // Rehome original schema references locally; no remote resolver is needed.
    let link = schema["$defs"]["link"].clone();
    let links =
        json!({"type": "array", "minItems": 1, "items": {"$ref": "#/components/schemas/Link"}});
    let mut landing = schema["$defs"]["landing"].clone();
    landing["properties"]["links"] = json!({"$ref": "#/components/schemas/Links"});
    let mut conformance = schema["$defs"]["conformance"].clone();
    conformance["properties"]["links"] = json!({"$ref": "#/components/schemas/Links"});
    let mut api = json!({
        "openapi": "3.1.0",
        "jsonSchemaDialect": "https://spec.openapis.org/oas/3.1/dialect/base",
        "info": {
            "title": "Glaux Server initial API",
            "version": env!("CARGO_PKG_VERSION"),
            "description": "Initial discovery and operational health only. No CSAPI resource operations, completed conformance classes or experiments are advertised. OpenAPI 3.1 availability is not a declaration of the separate OAS 3.0 class."
        },
        "servers": [{"url": boundary.link(&[], &[])?}],
        "paths": paths,
        "components": {"schemas": {
            "Link": link, "Links": links, "Landing": landing, "Conformance": conformance,
            "Problem": {
                "type": "object", "required": ["type", "title", "status", "detail", "correlation"],
                "properties": {
                    "type": {"type": "string", "format": "uri"},
                    "title": {"type": "string"},
                    "status": {"type": "integer", "minimum": 400, "maximum": 599},
                    "detail": {"type": "string"},
                    "correlation": {"type": "string", "description": "Server-minted diagnostic identifier, not an authorization token."}
                }
            }
        }}
    });
    if system_creation {
        api["info"]["description"] = json!(
            "Initial discovery, health and explicitly enabled minimal System creation and canonical retrieval. Collections, other resource operations and complete conformance classes are not advertised. OpenAPI 3.1 availability does not declare the separate OAS 3.0 class."
        );
        api["paths"][SYSTEM_CREATE.path()] =
            json!({"post": system_creation_operation(authentication)});
        api["paths"][SYSTEM_READ.path()] = json!({
            "get": system_read_operation(boundary, authentication, false)?,
            "head": system_read_operation(boundary, authentication, true)?
        });
    }
    if system_creation && authentication == Authentication::Jwt {
        api["components"]["securitySchemes"] = json!({
            "bearerAuth": {"type":"http", "scheme":"bearer", "bearerFormat":"JWT",
                "description":"Token verified under configured issuer, audience and trust; successful authentication alone grants no source/action permission."}
        });
    }
    Ok(api)
}

fn system_creation_operation(authentication: Authentication) -> Value {
    let mut operation = json!({
        "operationId": SYSTEM_CREATE.operation,
        "summary": SYSTEM_CREATE.title,
        "tags": [SYSTEM_CREATE.family.name()],
        "description": "Partial creation contract: one non-spatial GeoJSON System with uid, name and featureType. The configured ingestion source and verified caller determine authority, never body attribution. Optional content outside this documented subset is rejected, not silently discarded. Success is an empty 201; Accept does not select a resource body. Location identifies the new canonical resource, retrievable by GET under the same identity and permissions. This POST-only collection target currently has no representation or validator: If-Match fails, If-None-Match passes after authorization. Supplied local id and generated links are structurally checked then ignored as authority; original bytes remain restricted evidence.",
        "x-glaux-conformance-dependencies": [],
        "parameters": [{
            "name":"Idempotency-Key", "in":"header", "required":false,
            "description":"Optional Glaux retry extension, scoped to verified caller/configured source and this operation. Same exact intent within configured retention returns the original Location after reauthorization; different intent conflicts. Expiry permits new admission and does not guarantee deduplication.",
            "schema":{"type":"string", "minLength":1, "maxLength":256}
        }],
        "requestBody": {
            "required":true,
            "content":{"application/geo+json":{
                "schema":{
                    "type":"object", "required":["type","geometry","properties"],
                    "additionalProperties":false,
                    "properties":{
                        "type":{"const":"Feature"}, "geometry":{"type":"null"},
                        "id":{"oneOf":[{"type":"string","minLength":1},{"type":"number"}],"description":"Ignored after structural checking; the server generates its own UUIDv7 local identifier."},
                        "links":{"type":"array","minItems":1,"items":{"type":"object","required":["href"],"properties":{
                            "href":{"type":"string","format":"uri"}, "rel":{"type":"string"}, "type":{"type":"string"},
                            "hreflang":{"type":"string","minLength":1,"pattern":"^([a-z]{2}(-[A-Z]{2})?)|x-default$"},
                            "title":{"type":"string","minLength":1}, "uid":{"type":"string","format":"uri"},
                            "rt":{"type":"string","format":"uri"}, "if":{"type":"string","format":"uri"}
                        }},"description":"Optional generated-link input is checked using the pinned CSAPI link schema before removal; it supplies no stored association or authority."},
                        "properties":{
                            "type":"object", "required":["uid","name","featureType"], "additionalProperties":false,
                            "properties":{
                                "uid":{"type":"string","format":"uri","description":"Absolute URI, byte-preserved; Glaux limit 4096 UTF-8 bytes."},
                                "name":{"type":"string","minLength":1,"description":"Glaux limit 4096 UTF-8 bytes."},
                                "featureType":{"type":"string","enum":SYSTEM_TYPES}
                            }
                        }
                    }
                },
                "example":{"type":"Feature","geometry":null,"properties":{"uid":"urn:glaux:example:thermometer","name":"Example thermometer","featureType":"sosa:Sensor"}}
            }}
        },
        "responses":{
            "201":{"description":"Creation committed, or same-key same-intent retained outcome reauthorized. Empty body; no resource representation or ETag.","headers":{
                "Location":{"required":true,"schema":{"type":"string","format":"uri"},"description":"Configured public root plus /systems/{server-generated UUIDv7}; never derived from Host or forwarding headers."},
                "Cache-Control":{"schema":{"type":"string","const":"private, no-store"}}
            }},
            "default":{"description":"Problem Details:400 malformed input;401 credentials;403 source/action denied;409 UID or retry conflict;412 false precondition;413 request limit;415 unsupported media/coding;422 unsupported minimal-slice content;503 required dependency unavailable. No accepted resource/outgoing work on rejected pre-admission requests; bounded safe denial auditing is separate.","content":{"application/problem+json":{"schema":{"$ref":"#/components/schemas/Problem"}}}}
        }
    });
    operation["security"] = if authentication == Authentication::Jwt {
        json!([{"bearerAuth":[]}])
    } else {
        operation["x-glaux-development-identity"] = json!(
            "Explicit loopback-only configured identity; no caller credential header. Configured source/action permissions are still mandatory, and anonymous/disabled authentication cannot enable this operation."
        );
        json!([])
    };
    operation
}

fn system_read_operation(
    boundary: &HttpBoundary,
    authentication: Authentication,
    head: bool,
) -> Result<Value, Problem> {
    let example = json!({
        "type":"Feature", "id":EXAMPLE_SYSTEM_ID, "geometry":null,
        "properties":{"uid":"urn:glaux:example:thermometer","name":"Example thermometer","featureType":"sosa:Sensor"},
        "links":[{"href":boundary.link(&["systems", EXAMPLE_SYSTEM_ID], &[])?, "rel":"self", "type":"application/geo+json", "title":"This System"}]
    });
    let mut success = json!({
        "description":"The caller-visible current System. HEAD returns the GET headers without a body.",
        "headers":{
            "Cache-Control":{"schema":{"type":"string","const":"private, no-store"}},
            "Vary":{"schema":{"type":"string","const":"Accept"}}
        }
    });
    if !head {
        success["content"] = json!({"application/geo+json":{
            "schema":{
                "type":"object", "required":["type","id","geometry","properties","links"],
                "additionalProperties":false,
                "properties":{
                    "type":{"const":"Feature"},
                    "id":{"type":"string","description":"Server-generated UUIDv7 local identifier; the final segment of the creation Location."},
                    "geometry":{"type":"null"},
                    "properties":{
                        "type":"object", "required":["uid","name","featureType"], "additionalProperties":false,
                        "properties":{
                            "uid":{"type":"string","format":"uri"},
                            "name":{"type":"string","minLength":1},
                            "featureType":{"type":"string","enum":SYSTEM_TYPES,"description":"The exact spelling retained from the current accepted source."}
                        }
                    },
                    "links":{"type":"array","minItems":1,"items":{"$ref":"#/components/schemas/Link"},
                        "description":"A self link to the canonical URL, plus ogc-rel:parentSystem only for a visible parent. No collection or alternate-format link is offered."}
                }
            },
            "example":example
        }});
    }
    let mut error = json!({
        "description":"Problem Details: 401 missing or invalid credentials (Cache-Control no-store); otherwise Cache-Control private, no-store with 404 for a missing, concealed or non-canonical identifier (identical apart from correlation), 406 when application/geo+json is not acceptable, 500 when a stored System cannot be represented, 503 when a required dependency is unavailable."
    });
    if !head {
        error["content"] =
            json!({"application/problem+json":{"schema":{"$ref":"#/components/schemas/Problem"}}});
    }
    let mut operation = json!({
        "operationId": if head { "headSystem" } else { SYSTEM_READ.operation },
        "summary": SYSTEM_READ.title,
        "tags": [SYSTEM_READ.family.name()],
        "description": "Canonical retrieval of a System created through the minimal POST: exact identity, UID, name and retained featureType, with generated links. Requires Read permission for the System's creating source; a missing or concealed System returns the same 404. Standard HTTP Accept negotiation offers only application/geo+json, the default when Accept is absent. No ETag or conditional GET, SensorML, collection or alternate representation is offered in this increment.",
        "x-glaux-conformance-dependencies": [],
        "parameters": [{
            "name":"id", "in":"path", "required":true,
            "description":"Server-generated local identifier from the creation Location, in canonical lowercase form.",
            "schema":{"type":"string"}
        }],
        "responses": {"200": success, "default": error}
    });
    operation["security"] = if authentication == Authentication::Jwt {
        json!([{"bearerAuth":[]}])
    } else {
        operation["x-glaux-development-identity"] = json!(
            "Explicit loopback-only configured identity; no caller credential header. Configured source/action permissions are still mandatory."
        );
        json!([])
    };
    Ok(operation)
}

fn html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\'', "&#39;")
}

fn documentation(boundary: &HttpBoundary) -> Result<String, Problem> {
    let mut links = String::new();
    for route in [
        API,
        CONFORMANCE,
        SCHEMA,
        LANDING_EXAMPLE,
        CONFORMANCE_EXAMPLE,
        LICENSE,
        NOTICE,
        THIRD_PARTY,
    ] {
        links.push_str(&format!(
            "<li><a href=\"{}\">{}</a></li>",
            html(&boundary.link(route.segments, &[])?),
            html(route.title)
        ));
    }
    Ok(format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>Glaux Server API</title><link rel=\"stylesheet\" href=\"{}\"></head><body><h1>Glaux Server initial API</h1><p>Discovery and operational health only. No resource families or conformance classes are advertised yet. No credentials or Try-it-out controls are enabled.</p><ul>{links}</ul><div id=\"swagger-ui\"></div><script src=\"{}\" defer></script><script src=\"{}\" defer></script></body></html>",
        html(&boundary.link(CSS.segments, &[])?),
        html(&boundary.link(BUNDLE.segments, &[])?),
        html(&boundary.link(INIT.segments, &[])?)
    ))
}

fn initializer(boundary: &HttpBoundary) -> Result<String, Problem> {
    let url = serde_json::to_string(&boundary.link(API.segments, &[])?)
        .map_err(|_| Problem::internal())?;
    Ok(format!(
        "\"use strict\";\nwindow.addEventListener(\"DOMContentLoaded\", function () {{\n  SwaggerUIBundle({{url: {url}, dom_id: \"#swagger-ui\", layout: \"BaseLayout\", validatorUrl: null, supportedSubmitMethods: [], queryConfigEnabled: false, persistAuthorization: false, syntaxHighlight: false, deepLinking: false, displayRequestDuration: false, onComplete: function () {{ document.getElementById(\"swagger-ui\").setAttribute(\"data-glaux-rendered\", \"true\"); }} }});\n}});\n"
    ))
}

#[derive(Clone)]
enum Payload {
    Json(Arc<Value>),
    Bytes(Bytes),
}

impl Payload {
    fn json(value: Value) -> Self {
        Self::Json(Arc::new(value))
    }
    fn text(value: String) -> Self {
        Self::Bytes(Bytes::from(value))
    }
    fn static_bytes(value: &'static [u8]) -> Self {
        Self::Bytes(Bytes::from_static(value))
    }

    fn response(&self, route: RouteDefinition, headers: &HeaderMap) -> Result<Response, Problem> {
        negotiate(headers, &[route.media])?;
        let mut response = match self {
            Self::Json(value) => json_response(value, route.media)?,
            Self::Bytes(value) => (
                [
                    (header::CONTENT_TYPE, HeaderValue::from_static(route.media)),
                    (header::VARY, HeaderValue::from_static("Accept")),
                    (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
                ],
                value.clone(),
            )
                .into_response(),
        };
        response.headers_mut().insert(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(CSP),
        );
        response.headers_mut().insert(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        );
        response.headers_mut().insert(
            header::REFERRER_POLICY,
            HeaderValue::from_static("no-referrer"),
        );
        Ok(response)
    }
}

/// The caller opts in, merges this router with health, then wraps HttpBoundary.
/// All material is generated once from explicit configuration, never Host input.
pub fn router(boundary: &HttpBoundary) -> Result<Router, Problem> {
    router_with_system_creation(boundary, false, Authentication::Disabled)
}

/// The enable flag and authentication mode are the same validated values used
/// to install the creation handler. A POST-only route is not a browsable list.
pub fn router_with_system_creation(
    boundary: &HttpBoundary,
    system_creation: bool,
    authentication: Authentication,
) -> Result<Router, Problem> {
    let mut landing = landing(boundary)?;
    if system_creation {
        landing["description"] = json!(
            "Initial discovery and enabled minimal System creation and canonical retrieval, documented through the linked API definition. Collections and completed conformance classes are not advertised."
        );
    }
    let conformance = conformance(boundary)?;
    let mut schema: Value = serde_json::from_str(include_str!("../assets/discovery-schema.json"))
        .map_err(|_| Problem::internal())?;
    schema["$id"] = json!(boundary.link(SCHEMA.segments, &[])?);
    let description = openapi(boundary, &schema, system_creation, authentication)?;
    let mut documentation = documentation(boundary)?;
    if system_creation {
        documentation = documentation.replace(
            "Discovery and operational health only. No resource families or conformance classes are advertised yet. No credentials or Try-it-out controls are enabled.",
            "Discovery, health and minimal System creation and canonical retrieval. No collection or completed conformance class is advertised. Interactive submission remains disabled; both operations require the configured identity and source permissions.",
        );
    }
    let documents = [
        (LANDING, Payload::json(landing.clone())),
        (CONFORMANCE, Payload::json(conformance.clone())),
        (API, Payload::json(description)),
        (DOCS, Payload::text(documentation)),
        (INIT, Payload::text(initializer(boundary)?)),
        (
            BUNDLE,
            Payload::static_bytes(include_bytes!("../assets/swagger-ui/swagger-ui-bundle.js")),
        ),
        (
            CSS,
            Payload::static_bytes(include_bytes!("../assets/swagger-ui/swagger-ui.css")),
        ),
        (
            LICENSE,
            Payload::static_bytes(include_bytes!("../assets/swagger-ui/LICENSE")),
        ),
        (
            NOTICE,
            Payload::static_bytes(include_bytes!("../assets/swagger-ui/NOTICE")),
        ),
        (
            THIRD_PARTY,
            Payload::static_bytes(include_bytes!(
                "../assets/swagger-ui/swagger-ui-bundle.js.LICENSE.txt"
            )),
        ),
        (SCHEMA, Payload::json(schema)),
        (LANDING_EXAMPLE, Payload::json(landing)),
        (CONFORMANCE_EXAMPLE, Payload::json(conformance)),
    ];
    let mut router = Router::new();
    for (route, payload) in documents {
        router = router.route(
            route.path(),
            route.method(move |headers: HeaderMap| {
                let payload = payload.clone();
                async move { payload.response(route, &headers) }
            }),
        );
    }
    Ok(router)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http_boundary::Limits;
    use std::collections::HashSet;

    #[test]
    fn initial_route_definitions_are_unique_and_claim_no_classes() {
        assert_eq!(ROUTES.len(), 15);
        let mut paths = HashSet::new();
        let mut operations = HashSet::new();
        for route in ROUTES {
            assert!(paths.insert(route.path));
            assert!(operations.insert(route.operation));
            assert!(route.conformance_dependencies.is_empty());
        }
        let boundary = HttpBoundary::new(Some("https://example.test"), Limits::default()).unwrap();
        assert_eq!(conformance(&boundary).unwrap()["conformsTo"], json!([]));
    }

    #[test]
    fn generated_documents_keep_configured_prefix_and_local_references() {
        let boundary =
            HttpBoundary::new(Some("https://example.test/prefix"), Limits::default()).unwrap();
        let landing = landing(&boundary).unwrap();
        assert_eq!(
            landing["links"][1]["rel"],
            "http://www.opengis.net/def/rel/ogc/1.0/conformance"
        );
        assert_eq!(
            landing["links"][1]["href"],
            "https://example.test/prefix/conformance"
        );
        let schema = serde_json::from_str(include_str!("../assets/discovery-schema.json")).unwrap();
        let api = openapi(&boundary, &schema, false, Authentication::Disabled).unwrap();
        assert_eq!(api["servers"][0]["url"], "https://example.test/prefix");
        assert_eq!(api["paths"].as_object().unwrap().len(), 15);
        assert_eq!(
            api["paths"]["/health/ready"]["get"]["responses"]["503"]["content"]["text/plain; charset=utf-8"]
                ["schema"]["type"],
            "string"
        );
        assert!(
            api["paths"]["/api"]["head"]["responses"]["200"]
                .get("content")
                .is_none()
        );
        assert!(router(&HttpBoundary::new(None, Limits::default()).unwrap()).is_err());
    }

    #[test]
    fn documentation_is_external_local_and_escapes_markup() {
        assert_eq!(html("\"<&>'"), "&quot;&lt;&amp;&gt;&#39;");
        let boundary =
            HttpBoundary::new(Some("https://example.test/prefix"), Limits::default()).unwrap();
        let page = documentation(&boundary).unwrap();
        assert!(page.contains("src=\"https://example.test/prefix/docs/init.js\""));
        assert!(!page.contains("<script>"));
        let init = initializer(&boundary).unwrap();
        assert!(init.contains("url: \"https://example.test/prefix/api\""));
        assert!(init.contains("validatorUrl: null"));
        assert!(init.contains("supportedSubmitMethods: []"));
        assert!(init.contains("queryConfigEnabled: false"));
        assert!(!init.contains("location.search"));
    }

    #[test]
    fn system_creation_description_tracks_installation_and_authentication() {
        let boundary =
            HttpBoundary::new(Some("https://example.test/prefix"), Limits::default()).unwrap();
        let schema = serde_json::from_str(include_str!("../assets/discovery-schema.json")).unwrap();
        let disabled = openapi(&boundary, &schema, false, Authentication::Jwt).unwrap();
        assert!(disabled["paths"].get("/systems").is_none());
        assert!(disabled["paths"].get("/systems/{id}").is_none());
        for mode in [Authentication::Jwt, Authentication::Development] {
            let api = openapi(&boundary, &schema, true, mode).unwrap();
            assert_eq!(api["paths"].as_object().unwrap().len(), 17);
            let methods = api["paths"]["/systems"].as_object().unwrap();
            assert_eq!(
                methods.keys().map(String::as_str).collect::<Vec<_>>(),
                vec!["post"]
            );
            let member = api["paths"]["/systems/{id}"].as_object().unwrap();
            assert_eq!(
                member.keys().map(String::as_str).collect::<Vec<_>>(),
                vec!["get", "head"]
            );
            let get = &member["get"];
            assert_eq!(get["operationId"], "getSystem");
            assert_eq!(member["head"]["operationId"], "headSystem");
            assert_eq!(get["parameters"][0]["name"], "id");
            assert_eq!(get["parameters"][0]["in"], "path");
            let ok = &get["responses"]["200"];
            assert_eq!(
                ok["headers"]["Cache-Control"]["schema"]["const"],
                "private, no-store"
            );
            let content = ok["content"].as_object().unwrap();
            assert_eq!(
                content.keys().collect::<Vec<_>>(),
                vec!["application/geo+json"]
            );
            assert_eq!(
                content["application/geo+json"]["example"]["links"][0]["href"],
                "https://example.test/prefix/systems/0190f5c2-7b5a-7cc3-98c4-dc0c0c220001"
            );
            let head = &member["head"]["responses"]["200"];
            assert!(head.get("content").is_none());
            let post = &methods["post"];
            assert_eq!(get["security"], post["security"]);
            assert!(post["responses"]["201"].get("content").is_none());
            assert_eq!(
                post["responses"]["201"]["headers"]["Location"]["required"],
                true
            );
            assert_eq!(
                post["requestBody"]["content"]
                    .as_object()
                    .unwrap()
                    .keys()
                    .collect::<Vec<_>>(),
                vec!["application/geo+json"]
            );
            assert_eq!(
                post["requestBody"]["content"]["application/geo+json"]["example"],
                json!({
                    "type":"Feature", "geometry":null,
                    "properties":{"uid":"urn:glaux:example:thermometer","name":"Example thermometer","featureType":"sosa:Sensor"}
                })
            );
            if mode == Authentication::Jwt {
                assert_eq!(post["security"], json!([{"bearerAuth":[]}]));
                assert_eq!(
                    api["components"]["securitySchemes"]["bearerAuth"]["scheme"],
                    "bearer"
                );
            } else {
                assert_eq!(post["security"], json!([]));
                assert!(
                    post["x-glaux-development-identity"]
                        .as_str()
                        .unwrap()
                        .contains("loopback")
                );
            }
            assert_eq!(conformance(&boundary).unwrap()["conformsTo"], json!([]));
        }
    }
}
