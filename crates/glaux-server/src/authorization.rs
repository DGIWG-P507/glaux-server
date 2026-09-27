//! Configured action/source/resource admission for the initial System boundary.
//!
//! Future network handlers must use this boundary, not the trusted low-level
//! storage/application APIs. It neither creates resource routes nor interprets
//! producer assertions, source aliases or authentication as authority.

use crate::application::{
    self, AuditContext, AuditId, CreateSystem, DenialStorage, DeniedMutation, DeniedOperation,
    RetryKey, UpdateSystem, WriteReceipt,
};
use crate::authentication::{CallerContext, CallerKind};
use crate::authorization_storage;
pub use crate::authorization_storage::SystemPage;
use crate::http_boundary::Problem;
use crate::storage::{StorageError, SystemRecord};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use glaux_domain::identity::LocalId;
use glaux_domain::temporal::ExactInstant;
use serde::Deserialize;
use serde_json::json;
use sqlx::PgConnection;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Read,
    Create,
    Update,
    SubmitCommand,
    ReportStatus,
    Publish,
    Export,
    Administer,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyConfig {
    pub grants: Vec<GrantConfig>,
    pub denial_audit: DenialLimits,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrantConfig {
    pub issuer: String,
    pub subject: Option<String>,
    pub group: Option<String>,
    pub source: String,
    pub actions: Vec<Action>,
    pub resources: Option<Vec<String>>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DenialLimits {
    pub max_records: u32,
    pub max_per_window: u32,
    pub window_seconds: u32,
}

impl DenialLimits {
    pub fn validate(self) -> Result<Self, PolicyError> {
        if !(1..=100_000).contains(&self.max_records)
            || !(1..=10_000).contains(&self.max_per_window)
            || !(1..=86_400).contains(&self.window_seconds)
        {
            return Err(PolicyError);
        }
        Ok(self)
    }
}

impl Default for DenialLimits {
    fn default() -> Self {
        Self {
            max_records: 10_000,
            max_per_window: 60,
            window_seconds: 60,
        }
    }
}

/// No policy contents, credentials or caller values enter this diagnostic.
#[derive(Clone, Copy, Debug)]
pub struct PolicyError;

impl fmt::Display for PolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("policy configuration or decision is unavailable")
    }
}
impl std::error::Error for PolicyError {}

#[derive(Clone)]
struct ResourceGrant {
    source: String,
    resources: Option<Vec<LocalId>>,
}

/// Cannot be constructed from unchecked request claims. Sources and resource
/// IDs stay paired; flattening them into independent sets would expand access.
#[derive(Clone, Default)]
pub struct PermissionSet {
    grants: Vec<ResourceGrant>,
}

impl PermissionSet {
    pub fn allows(&self, source: &str, id: LocalId) -> bool {
        self.grants.iter().any(|grant| {
            let matching_source = grant.source == source; // SOURCE_PERMISSION_COMPARISON
            matching_source
                && grant
                    .resources
                    .as_ref()
                    .is_none_or(|resources| resources.contains(&id))
        })
    }

    fn allows_source(&self, source: &str) -> bool {
        self.grants.iter().any(|grant| {
            let matching_source = grant.source == source; // SOURCE_PREFLIGHT_PERMISSION_COMPARISON
            matching_source
                && grant
                    .resources
                    .as_ref()
                    .is_none_or(|resources| !resources.is_empty())
        })
    }

    fn scope_json(&self) -> String {
        json!(
            self.grants
                .iter()
                .map(|grant| json!({
                    "source": grant.source,
                    "resources": grant.resources.as_ref().map(|resources| {
                        resources.iter().map(ToString::to_string).collect::<Vec<_>>()
                    }),
                }))
                .collect::<Vec<_>>()
        )
        .to_string()
    }
}

/// Local, bounded decisions only. A remote integration needs its own explicitly
/// bounded adapter contract; no asynchronous network call is hidden here.
pub trait AccessPolicy: Send + Sync {
    fn permissions(
        &self,
        caller: &CallerContext,
        action: Action,
    ) -> Result<PermissionSet, PolicyError>;
}

#[derive(Clone)]
struct PolicyGrant {
    issuer: String,
    subject: Option<String>,
    group: Option<String>,
    actions: Vec<Action>,
    scope: ResourceGrant,
}

#[derive(Clone)]
pub struct ConfiguredPolicy {
    grants: Vec<PolicyGrant>,
    limits: DenialLimits,
}

fn bounded_text(value: &str, maximum: usize) -> bool {
    !value.is_empty() && value.len() <= maximum && !value.chars().any(char::is_control)
}

impl ConfiguredPolicy {
    pub fn new(config: PolicyConfig) -> Result<Self, PolicyError> {
        let limits = config.denial_audit.validate()?;
        if config.grants.len() > 256 {
            return Err(PolicyError);
        }
        let mut grants = Vec::new();
        let mut total_resources = 0usize;
        for grant in config.grants {
            if !bounded_text(&grant.issuer, 1024)
                || !bounded_text(&grant.source, 256)
                || grant.subject.is_some() == grant.group.is_some()
                || grant
                    .subject
                    .as_deref()
                    .is_some_and(|v| !bounded_text(v, 1024))
                || grant
                    .group
                    .as_deref()
                    .is_some_and(|v| !bounded_text(v, 256))
                || grant.actions.is_empty()
                || grant.actions.len() > 8
                || grant
                    .actions
                    .iter()
                    .enumerate()
                    .any(|(index, action)| grant.actions[..index].contains(action))
            {
                return Err(PolicyError);
            }
            let resources = grant
                .resources
                .map(|values| {
                    total_resources = total_resources.saturating_add(values.len());
                    if values.len() > 1024 || total_resources > 4096 {
                        return Err(PolicyError);
                    }
                    let mut ids = Vec::new();
                    for value in values {
                        let id = value.parse::<LocalId>().map_err(|_| PolicyError)?;
                        if ids.contains(&id) {
                            return Err(PolicyError);
                        }
                        ids.push(id);
                    }
                    Ok(ids)
                })
                .transpose()?;
            grants.push(PolicyGrant {
                issuer: grant.issuer,
                subject: grant.subject,
                group: grant.group,
                actions: grant.actions,
                scope: ResourceGrant {
                    source: grant.source,
                    resources,
                },
            });
        }
        Ok(Self { grants, limits })
    }

    pub fn deny_all() -> Self {
        Self {
            grants: Vec::new(),
            limits: DenialLimits::default(),
        }
    }

    pub fn limits(&self) -> DenialLimits {
        self.limits
    }
}

impl AccessPolicy for ConfiguredPolicy {
    fn permissions(
        &self,
        caller: &CallerContext,
        action: Action,
    ) -> Result<PermissionSet, PolicyError> {
        let granted = PermissionSet {
            grants: self
                .grants
                .iter()
                .filter(|grant| {
                    grant.issuer == caller.issuer()
                        && grant.actions.contains(&action)
                        && (grant.subject.as_deref() == Some(caller.subject())
                            || grant
                                .group
                                .as_ref()
                                .is_some_and(|group| caller.groups().contains(group)))
                })
                .map(|grant| grant.scope.clone())
                .collect(),
        };
        Ok(granted)
    }
}

pub trait RateClock: Send + Sync {
    fn now(&self) -> Option<Duration>;
}

pub struct SystemRateClock {
    started: Instant,
}
impl Default for SystemRateClock {
    fn default() -> Self {
        Self {
            started: Instant::now(),
        }
    }
}
impl RateClock for SystemRateClock {
    fn now(&self) -> Option<Duration> {
        Some(self.started.elapsed())
    }
}

/// Fixed-cardinality protected diagnostics, not an HTTP response or audit API.
/// Saturating counters never contain caller/source/target/reason labels.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Diagnostics {
    pub retained: u64,
    pub rate_limited: u64,
    pub storage_limited: u64,
    pub storage_busy: u64,
    pub storage_failed: u64,
    pub clock_unavailable: u64,
}

#[derive(Default)]
struct Counters {
    retained: AtomicU64,
    rate_limited: AtomicU64,
    storage_limited: AtomicU64,
    storage_busy: AtomicU64,
    storage_failed: AtomicU64,
    clock_unavailable: AtomicU64,
}

fn increment(counter: &AtomicU64) {
    let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
        Some(value.saturating_add(1))
    });
}

#[derive(Default)]
struct RateState {
    start: Option<Duration>,
    last: Option<Duration>,
    used: u32,
}

struct DenialState {
    limits: DenialLimits,
    clock: Arc<dyn RateClock>,
    rate: Mutex<RateState>,
    counts: Counters,
}

impl DenialState {
    fn take_capacity(&self) -> bool {
        let Some(now) = self.clock.now() else {
            increment(&self.counts.clock_unavailable);
            return false;
        };
        let Ok(mut state) = self.rate.lock() else {
            increment(&self.counts.clock_unavailable);
            return false;
        };
        if state.last.is_some_and(|last| now < last) {
            increment(&self.counts.clock_unavailable);
            return false;
        }
        state.last = Some(now);
        if state.start.is_none_or(|start| {
            now.saturating_sub(start) >= Duration::from_secs(u64::from(self.limits.window_seconds))
        }) {
            state.start = Some(now);
            state.used = 0;
        }
        if state.used >= self.limits.max_per_window {
            increment(&self.counts.rate_limited);
            return false;
        }
        state.used += 1;
        true
    }
}

/// Produced only from a verified CallerContext and trusted receipt time. It
/// mints its own correlation rather than admitting a request's audit metadata.
pub struct OperationContext {
    caller: CallerContext,
    actor: Option<String>,
    time: ExactInstant,
    correlation: String,
}

impl OperationContext {
    pub fn new(caller: CallerContext, time: ExactInstant) -> Result<Self, AccessError> {
        let correlation = LocalId::generate()
            .map(|id| id.to_string())
            .map_err(|_| AccessError::new(AccessKind::Unavailable, "unavailable"))?;
        let kind = match caller.kind() {
            CallerKind::Jwt => "jwt",
            CallerKind::Development => "development",
        };
        let actor = serde_json::to_string(&[caller.issuer(), caller.subject(), kind])
            .map_err(|_| AccessError::new(AccessKind::Unavailable, &correlation))?;
        // Never truncate or hash different identities into the same audit actor.
        // Oversized identities remain usable for reads; accepted writes fail.
        let actor = (actor.len() <= 256).then_some(actor);
        Ok(Self {
            caller,
            actor,
            time,
            correlation,
        })
    }

    pub fn caller(&self) -> &CallerContext {
        &self.caller
    }

    pub fn correlation(&self) -> &str {
        &self.correlation
    }

    fn audit(&self, source: Option<&str>) -> AuditContext {
        AuditContext {
            actor: self.actor.clone(),
            source: source.map(str::to_owned),
            correlation: self.correlation.clone(),
            time: self.time.clone(),
        }
    }

    fn error(&self, kind: AccessKind) -> AccessError {
        AccessError::new(kind, &self.correlation)
    }
}

#[derive(Clone, Copy, Debug)]
enum AccessKind {
    Denied,
    NotFound,
    Unavailable,
    BadRequest,
    Conflict,
    Precondition,
}

#[derive(Clone, Debug)]
pub struct AccessError {
    kind: AccessKind,
    correlation: String,
}

impl AccessError {
    fn new(kind: AccessKind, correlation: &str) -> Self {
        Self {
            kind,
            correlation: correlation.to_owned(),
        }
    }

    pub fn status(&self) -> StatusCode {
        match self.kind {
            AccessKind::Denied => StatusCode::FORBIDDEN,
            AccessKind::NotFound => StatusCode::NOT_FOUND,
            AccessKind::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
            AccessKind::BadRequest => StatusCode::BAD_REQUEST,
            AccessKind::Conflict => StatusCode::CONFLICT,
            AccessKind::Precondition => StatusCode::PRECONDITION_FAILED,
        }
    }

    pub fn correlation(&self) -> &str {
        &self.correlation
    }
}

impl fmt::Display for AccessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self.kind {
            AccessKind::Denied => "operation not permitted",
            AccessKind::NotFound => "resource unavailable",
            AccessKind::Unavailable => "operation temporarily unavailable",
            AccessKind::BadRequest => "invalid operation input",
            AccessKind::Conflict => "operation conflicts with retained state",
            AccessKind::Precondition => "supplied condition failed",
        })
    }
}
impl std::error::Error for AccessError {}

impl IntoResponse for AccessError {
    fn into_response(self) -> Response {
        let problem = match self.kind {
            AccessKind::Denied => Problem::forbidden(),
            AccessKind::NotFound => Problem::not_found(),
            AccessKind::Unavailable => Problem::unavailable(),
            AccessKind::BadRequest => Problem::bad_request(),
            AccessKind::Conflict => Problem::conflict(),
            AccessKind::Precondition => Problem::precondition_failed(),
        };
        problem.into_response_with_correlation(&self.correlation)
    }
}

#[derive(Clone)]
pub struct Admission {
    policy: Arc<dyn AccessPolicy>,
    denial: Arc<DenialState>,
}

impl Admission {
    pub fn new(
        policy: Arc<dyn AccessPolicy>,
        limits: DenialLimits,
        clock: Arc<dyn RateClock>,
    ) -> Result<Self, PolicyError> {
        Ok(Self {
            policy,
            denial: Arc::new(DenialState {
                limits: limits.validate()?,
                clock,
                rate: Mutex::new(RateState::default()),
                counts: Counters::default(),
            }),
        })
    }

    pub fn diagnostics(&self) -> Diagnostics {
        let counts = &self.denial.counts;
        Diagnostics {
            retained: counts.retained.load(Ordering::Relaxed),
            rate_limited: counts.rate_limited.load(Ordering::Relaxed),
            storage_limited: counts.storage_limited.load(Ordering::Relaxed),
            storage_busy: counts.storage_busy.load(Ordering::Relaxed),
            storage_failed: counts.storage_failed.load(Ordering::Relaxed),
            clock_unavailable: counts.clock_unavailable.load(Ordering::Relaxed),
        }
    }

    fn permissions(
        &self,
        ctx: &OperationContext,
        action: Action,
    ) -> Result<PermissionSet, AccessError> {
        self.policy
            .permissions(&ctx.caller, action)
            .map_err(|_| ctx.error(AccessKind::Unavailable))
    }

    fn storage_error(ctx: &OperationContext, error: StorageError) -> AccessError {
        let kind = match error {
            StorageError::Denied => AccessKind::Denied,
            StorageError::NotFound => AccessKind::NotFound,
            StorageError::Conflict | StorageError::InvalidAssociation => AccessKind::Conflict,
            StorageError::PreconditionFailed => AccessKind::Precondition,
            StorageError::InvalidInput => AccessKind::BadRequest,
            _ => AccessKind::Unavailable,
        };
        ctx.error(kind)
    }

    pub async fn list_systems(
        &self,
        connection: &mut PgConnection,
        ctx: &OperationContext,
        limit: u16,
    ) -> Result<SystemPage, AccessError> {
        let scope = self.permissions(ctx, Action::Read)?;
        authorization_storage::list_systems(connection, &scope.scope_json(), None, limit)
            .await
            .map_err(|error| Self::storage_error(ctx, error))
    }

    pub async fn get_system(
        &self,
        connection: &mut PgConnection,
        ctx: &OperationContext,
        id: LocalId,
    ) -> Result<SystemRecord, AccessError> {
        let scope = self.permissions(ctx, Action::Read)?;
        let mut page =
            authorization_storage::list_systems(connection, &scope.scope_json(), Some(id), 1)
                .await
                .map_err(|error| Self::storage_error(ctx, error))?;
        page.items
            .pop()
            .ok_or_else(|| ctx.error(AccessKind::NotFound))
    }

    async fn deny(
        &self,
        connection: &mut PgConnection,
        ctx: &OperationContext,
        operation: DeniedOperation,
        kind: AccessKind,
        safe_target: Option<(LocalId, &str)>,
    ) -> AccessError {
        if !self.denial.take_capacity() {
            return ctx.error(kind);
        }
        let Ok(audit_id) = AuditId::generate() else {
            increment(&self.denial.counts.storage_failed);
            return ctx.error(kind);
        };
        // Only a target already established as wholly readable may be retained.
        // Its source is the immutable CREATE owner, not request attribution.
        // Concealed/cross-source attempts never trigger an audit-only lookup.
        let denial = DeniedMutation {
            audit_id,
            target: safe_target.map(|(id, _)| id),
            audit: ctx.audit(safe_target.map(|(_, source)| source)),
            operation,
        };
        let counter = match application::record_denied_mutation_bounded(
            connection,
            &denial,
            self.denial.limits.max_records,
        )
        .await
        {
            Ok(DenialStorage::Retained) => &self.denial.counts.retained,
            Ok(DenialStorage::Capacity) => &self.denial.counts.storage_limited,
            Ok(DenialStorage::Busy) => &self.denial.counts.storage_busy,
            Err(_) => &self.denial.counts.storage_failed,
        };
        increment(counter);
        ctx.error(kind)
    }

    /// Cheap action/source screening before JSON and schema processing. It does
    /// not authorize a candidate or saved retry outcome; create_system_if repeats
    /// those actual-resource checks under the existing transaction boundary.
    pub(crate) async fn preflight_system_create(
        &self,
        connection: &mut PgConnection,
        ctx: &OperationContext,
        source: &str,
    ) -> Result<(), AccessError> {
        let create = self.permissions(ctx, Action::Create)?;
        if !bounded_text(source, 256) || !create.allows_source(source) {
            return Err(self
                .deny(
                    connection,
                    ctx,
                    DeniedOperation::Create,
                    AccessKind::Denied,
                    None,
                )
                .await);
        }
        if ctx.actor.is_none() {
            return Err(ctx.error(AccessKind::Unavailable));
        }
        Ok(())
    }

    pub async fn create_system(
        &self,
        connection: &mut PgConnection,
        ctx: &OperationContext,
        source: &str,
        input: CreateSystem,
        retry: Option<&RetryKey>,
    ) -> Result<WriteReceipt, AccessError> {
        self.create_system_if(connection, ctx, source, input, retry, true)
            .await
    }

    /// HTTP conditions describe the request target, not the candidate identity.
    /// Check only after authorizing the selected candidate or retained receipt.
    pub(crate) async fn create_system_if(
        &self,
        connection: &mut PgConnection,
        ctx: &OperationContext,
        source: &str,
        mut input: CreateSystem,
        retry: Option<&RetryKey>,
        precondition_satisfied: bool,
    ) -> Result<WriteReceipt, AccessError> {
        let create = self.permissions(ctx, Action::Create)?;
        // A fresh candidate ID is not part of retry intent. Only the callback
        // knows whether this attempt selects an original receipt or creates the
        // candidate; it authorizes that selected ID before comparing intent.
        let permitted_scope = if retry.is_some() {
            create.allows_source(source)
        } else {
            create.allows(source, input.system.id)
        };
        if !bounded_text(source, 256) || !permitted_scope {
            return Err(self
                .deny(
                    connection,
                    ctx,
                    DeniedOperation::Create,
                    AccessKind::Denied,
                    None,
                )
                .await);
        }
        if ctx.actor.is_none() {
            return Err(ctx.error(AccessKind::Unavailable));
        }
        let read = self.permissions(ctx, Action::Read)?;
        let parent = if let Some(parent) = input.system.parent {
            let page = authorization_storage::list_systems(
                connection,
                &read.scope_json(),
                Some(parent),
                1,
            )
            .await
            .map_err(|error| Self::storage_error(ctx, error))?;
            let parent_source = authorization_storage::system_source(connection, parent)
                .await
                .map_err(|error| Self::storage_error(ctx, error))?;
            match (parent_source, page.items.first()) {
                (Some(parent_source), Some(record))
                    if retry.is_some() || create.allows(&parent_source, parent) =>
                {
                    let parent_link = if let Some(link) = record.parent {
                        let Some(link_source) =
                            authorization_storage::system_source(connection, link)
                                .await
                                .map_err(|error| Self::storage_error(ctx, error))?
                        else {
                            return Err(self
                                .deny(
                                    connection,
                                    ctx,
                                    DeniedOperation::Create,
                                    AccessKind::Denied,
                                    None,
                                )
                                .await);
                        };
                        Some((link, link_source))
                    } else {
                        None
                    };
                    Some((parent, parent_source, parent_link))
                }
                _ => {
                    return Err(self
                        .deny(
                            connection,
                            ctx,
                            DeniedOperation::Create,
                            AccessKind::Denied,
                            None,
                        )
                        .await);
                }
            }
        } else {
            None
        };
        input.audit = ctx.audit(Some(source));
        input.revision.receipt_time = ctx.time.clone();
        let mut callback_error = AccessKind::Denied;
        let result = application::create_system_with_retry_admission(
            connection,
            &input,
            retry,
            |receipt, replay| {
                let (Ok(create), Ok(read)) = (
                    self.policy.permissions(&ctx.caller, Action::Create),
                    self.policy.permissions(&ctx.caller, Action::Read),
                ) else {
                    callback_error = AccessKind::Unavailable;
                    return false;
                };
                let permitted = create.allows(source, receipt.system_id)
                    && (!replay || read.allows(source, receipt.system_id))
                    && parent.as_ref().is_none_or(|(id, owner, link)| {
                        (replay || create.allows(owner, *id))
                            && read.allows(owner, *id)
                            && link
                                .as_ref()
                                .is_none_or(|(id, owner)| read.allows(owner, *id))
                    });
                if permitted && !precondition_satisfied {
                    callback_error = AccessKind::Precondition;
                    return false;
                }
                permitted
            },
        )
        .await;
        match result {
            Ok(receipt) => Ok(receipt),
            Err(StorageError::Denied) if matches!(callback_error, AccessKind::Precondition) => {
                Err(ctx.error(AccessKind::Precondition))
            }
            Err(StorageError::Denied) if matches!(callback_error, AccessKind::Unavailable) => {
                Err(ctx.error(AccessKind::Unavailable))
            }
            Err(StorageError::Denied) => Err(self
                .deny(
                    connection,
                    ctx,
                    DeniedOperation::Create,
                    AccessKind::Denied,
                    None,
                )
                .await),
            Err(error) => Err(Self::storage_error(ctx, error)),
        }
    }

    /// The initial receipt-returning update requires Read as well as Update so
    /// existing identity, parent and revision facts cannot be disclosed through
    /// its outcome. This is this wrapper's contract, not a universal CSAPI rule.
    pub async fn update_system(
        &self,
        connection: &mut PgConnection,
        ctx: &OperationContext,
        mut input: UpdateSystem,
    ) -> Result<WriteReceipt, AccessError> {
        let read = self.permissions(ctx, Action::Read)?;
        let update = self.permissions(ctx, Action::Update)?;
        let id = input.system_id;
        let page = authorization_storage::list_systems(connection, &read.scope_json(), Some(id), 1)
            .await
            .map_err(|error| Self::storage_error(ctx, error))?;
        if page.items.is_empty() {
            return Err(self
                .deny(
                    connection,
                    ctx,
                    DeniedOperation::Update,
                    AccessKind::NotFound,
                    None,
                )
                .await);
        }
        let Some(source) = authorization_storage::system_source(connection, id)
            .await
            .map_err(|error| Self::storage_error(ctx, error))?
        else {
            return Err(self
                .deny(
                    connection,
                    ctx,
                    DeniedOperation::Update,
                    AccessKind::NotFound,
                    None,
                )
                .await);
        };
        if !update.allows(&source, id) {
            return Err(self
                .deny(
                    connection,
                    ctx,
                    DeniedOperation::Update,
                    AccessKind::Denied,
                    Some((id, &source)),
                )
                .await);
        }
        if ctx.actor.is_none() {
            return Err(ctx.error(AccessKind::Unavailable));
        }
        let parent = if let Some(parent_id) = page.items.first().and_then(|record| record.parent) {
            let Some(parent_source) = authorization_storage::system_source(connection, parent_id)
                .await
                .map_err(|error| Self::storage_error(ctx, error))?
            else {
                return Err(self
                    .deny(
                        connection,
                        ctx,
                        DeniedOperation::Update,
                        AccessKind::NotFound,
                        None,
                    )
                    .await);
            };
            Some((parent_id, parent_source))
        } else {
            None
        };
        input.audit = ctx.audit(Some(&source));
        input.revision.receipt_time = ctx.time.clone();
        let mut callback_error = AccessKind::Denied;
        let result = application::update_system_admission(connection, &input, || {
            let (Ok(read), Ok(update)) = (
                self.policy.permissions(&ctx.caller, Action::Read),
                self.policy.permissions(&ctx.caller, Action::Update),
            ) else {
                callback_error = AccessKind::Unavailable;
                return false;
            };
            if !read.allows(&source, id)
                || parent
                    .as_ref()
                    .is_some_and(|(id, owner)| !read.allows(owner, *id))
            {
                callback_error = AccessKind::NotFound;
                return false;
            }
            update.allows(&source, id)
        })
        .await;
        match result {
            Ok(receipt) => Ok(receipt),
            Err(StorageError::Denied) if matches!(callback_error, AccessKind::Unavailable) => {
                Err(ctx.error(AccessKind::Unavailable))
            }
            Err(StorageError::Denied) => {
                let safe_target =
                    matches!(callback_error, AccessKind::Denied).then_some((id, source.as_str()));
                Err(self
                    .deny(
                        connection,
                        ctx,
                        DeniedOperation::Update,
                        callback_error,
                        safe_target,
                    )
                    .await)
            }
            Err(error) => Err(Self::storage_error(ctx, error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authentication::{Authenticator, DevelopmentConfig};
    use axum::http::HeaderMap;

    fn id(number: u32) -> LocalId {
        format!("01890f20-7b5a-7cc3-98c4-{number:012x}")
            .parse()
            .unwrap()
    }

    fn caller(subject: &str, groups: &[&str]) -> CallerContext {
        let peer = "127.0.0.1:12345".parse().unwrap();
        Authenticator::development(
            DevelopmentConfig {
                subject: subject.to_owned(),
                groups: groups.iter().map(|value| (*value).to_owned()).collect(),
                scopes: Vec::new(),
            },
            peer,
        )
        .unwrap()
        .authenticate(&HeaderMap::new(), Some(peer))
        .unwrap()
    }

    fn grant(source: &str, actions: &[Action], resources: Option<Vec<String>>) -> GrantConfig {
        GrantConfig {
            issuer: "urn:glaux:development".to_owned(),
            subject: Some("alice".to_owned()),
            group: None,
            source: source.to_owned(),
            actions: actions.to_vec(),
            resources,
        }
    }

    fn configuration(grants: Vec<GrantConfig>) -> PolicyConfig {
        PolicyConfig {
            grants,
            denial_audit: DenialLimits::default(),
        }
    }

    #[test]
    fn configured_policy_keeps_identity_actions_and_resource_pairs_distinct() {
        let alice = caller("alice", &["operators"]);
        let bob = caller("bob", &["operators"]);
        let wrong_group = caller("bob", &["Operators"]);
        let mut group = grant("source-B", &[Action::Read], Some(vec![id(2).to_string()]));
        group.subject = None;
        group.group = Some("operators".to_owned());
        let policy = ConfiguredPolicy::new(configuration(vec![
            grant("source-A", &[Action::Read], Some(vec![id(1).to_string()])),
            group,
            grant(
                "source-A",
                &[Action::SubmitCommand],
                Some(vec![id(1).to_string()]),
            ),
            grant("source-C", &[Action::Publish], None),
            grant("source-D", &[Action::Read], Some(Vec::new())),
        ]))
        .unwrap();
        let read = policy.permissions(&alice, Action::Read).unwrap();
        assert!(read.allows("source-A", id(1)));
        assert!(read.allows("source-B", id(2)));
        assert!(!read.allows("source-A", id(2)));
        assert!(!read.allows("source-B", id(1)));
        assert!(!read.allows("source-a", id(1)));
        assert!(!read.allows("source-A", id(3)));
        assert!(!read.allows("source-D", id(1)));
        assert!(!read.allows_source("source-D"));
        let bob_read = policy.permissions(&bob, Action::Read).unwrap();
        assert!(!bob_read.allows("source-A", id(1)));
        assert!(bob_read.allows("source-B", id(2)));
        assert!(
            !policy
                .permissions(&wrong_group, Action::Read)
                .unwrap()
                .allows("source-B", id(2))
        );
        assert!(
            policy
                .permissions(&alice, Action::SubmitCommand)
                .unwrap()
                .allows("source-A", id(1))
        );
        for action in [
            Action::Create,
            Action::Update,
            Action::ReportStatus,
            Action::Export,
            Action::Administer,
        ] {
            assert!(
                !policy
                    .permissions(&alice, action)
                    .unwrap()
                    .allows("source-A", id(1))
            );
        }
        let publish = policy.permissions(&alice, Action::Publish).unwrap();
        assert!(publish.allows("source-C", id(1)));
        assert!(publish.allows("source-C", id(999)));
        assert!(!publish.allows("source-B", id(999)));
        let mut other_issuer = grant("source-A", &[Action::Read], None);
        other_issuer.issuer = "urn:another:issuer".to_owned();
        let other = ConfiguredPolicy::new(configuration(vec![other_issuer])).unwrap();
        assert!(
            !other
                .permissions(&alice, Action::Read)
                .unwrap()
                .allows("source-A", id(1))
        );
        assert!(
            !ConfiguredPolicy::deny_all()
                .permissions(&alice, Action::Read)
                .unwrap()
                .allows("source-A", id(1))
        );
        let scope: serde_json::Value = serde_json::from_str(&read.scope_json()).unwrap();
        assert_eq!(
            scope,
            json!([
                {"source":"source-A","resources":["01890f20-7b5a-7cc3-98c4-000000000001"]},
                {"source":"source-B","resources":["01890f20-7b5a-7cc3-98c4-000000000002"]},
                {"source":"source-D","resources":[]},
            ])
        );
    }

    #[test]
    fn policy_configuration_and_denial_bounds_reject_ambiguity() {
        let valid = grant("source-A", &[Action::Read], None);
        for change in 0..12 {
            let mut candidate = valid.clone();
            match change {
                0 => candidate.issuer.clear(),
                1 => candidate.issuer = "unsafe\nissuer".to_owned(),
                2 => candidate.subject = None,
                3 => candidate.group = Some("operators".to_owned()),
                4 => candidate.subject = Some(String::new()),
                5 => {
                    candidate.subject = None;
                    candidate.group = Some(String::new());
                }
                6 => candidate.source.clear(),
                7 => candidate.source = "x".repeat(257),
                8 => candidate.actions.clear(),
                9 => candidate.actions.push(Action::Read),
                10 => candidate.resources = Some(vec![id(1).to_string().to_uppercase()]),
                11 => candidate.resources = Some(vec![id(1).to_string(), id(1).to_string()]),
                _ => unreachable!(),
            }
            assert!(ConfiguredPolicy::new(configuration(vec![candidate])).is_err());
        }
        assert!(ConfiguredPolicy::new(configuration(vec![valid.clone(); 256])).is_ok());
        assert!(ConfiguredPolicy::new(configuration(vec![valid; 257])).is_err());
        let mut large: Vec<_> = (0..4)
            .map(|group| {
                grant(
                    &format!("source-{group}"),
                    &[Action::Read],
                    Some(
                        (1..=1024)
                            .map(|offset| id(group * 1024 + offset).to_string())
                            .collect(),
                    ),
                )
            })
            .collect();
        assert!(ConfiguredPolicy::new(configuration(large.clone())).is_ok());
        large.push(grant(
            "extra",
            &[Action::Read],
            Some(vec![id(4097).to_string()]),
        ));
        assert!(ConfiguredPolicy::new(configuration(large)).is_err());
        assert!(
            ConfiguredPolicy::new(configuration(vec![grant(
                "too-many",
                &[Action::Read],
                Some((1..=1025).map(|value| id(value).to_string()).collect()),
            )]))
            .is_err()
        );
        for limits in [
            DenialLimits {
                max_records: 1,
                max_per_window: 1,
                window_seconds: 1,
            },
            DenialLimits {
                max_records: 100_000,
                max_per_window: 10_000,
                window_seconds: 86_400,
            },
        ] {
            assert!(limits.validate().is_ok());
        }
        for change in 0..6 {
            let mut limits = DenialLimits::default();
            match change {
                0 => limits.max_records = 0,
                1 => limits.max_records = 100_001,
                2 => limits.max_per_window = 0,
                3 => limits.max_per_window = 10_001,
                4 => limits.window_seconds = 0,
                5 => limits.window_seconds = 86_401,
                _ => unreachable!(),
            }
            assert!(
                ConfiguredPolicy::new(PolicyConfig {
                    grants: Vec::new(),
                    denial_audit: limits
                })
                .is_err()
            );
        }
        for malformed in [
            json!({"grants":[]}),
            json!({"grants":[],"denial_audit":null}),
            json!({"grants":[],"denial_audit":{"max_records":1,"max_per_window":1,"window_seconds":1},"allow":true}),
            json!({"grants":[],"denial_audit":{"max_records":1,"max_per_window":1,"window_seconds":1,"extra":true}}),
        ] {
            assert!(serde_json::from_value::<PolicyConfig>(malformed).is_err());
        }
        assert!(serde_json::from_value::<Action>(json!("administrator")).is_err());
    }

    #[test]
    fn operation_context_does_not_truncate_oversized_audit_identity() {
        let time = "2026-09-26T12:00:00.1234567890123456789Z"
            .parse::<ExactInstant>()
            .unwrap();
        let ordinary = OperationContext::new(caller("alice", &[]), time.clone()).unwrap();
        assert_eq!(
            ordinary.actor.as_deref(),
            Some("[\"urn:glaux:development\",\"alice\",\"development\"]")
        );
        assert!(ordinary.correlation().parse::<LocalId>().is_ok());
        let audit = ordinary.audit(Some("source-A"));
        assert_eq!(audit.source.as_deref(), Some("source-A"));
        assert_eq!(audit.correlation, ordinary.correlation());
        assert_eq!(
            audit.time.source_lexeme(),
            "2026-09-26T12:00:00.1234567890123456789Z"
        );
        let subject = "long-verified-subject-".repeat(20);
        let long = OperationContext::new(caller(&subject, &[]), time).unwrap();
        assert!(long.actor.is_none());
        assert_eq!(long.caller().subject(), subject);
        let mut read_grant = grant("source-A", &[Action::Read], None);
        read_grant.subject = Some(subject);
        let policy = ConfiguredPolicy::new(configuration(vec![read_grant])).unwrap();
        assert!(
            policy
                .permissions(long.caller(), Action::Read)
                .unwrap()
                .allows("source-A", id(1))
        );
        assert!(long.audit(None).actor.is_none());
        let error = long.error(AccessKind::Unavailable);
        assert_eq!(error.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(error.correlation(), long.correlation());
        assert_eq!(error.to_string(), "operation temporarily unavailable");
    }

    struct ManualClock(Mutex<Option<Duration>>);

    impl RateClock for ManualClock {
        fn now(&self) -> Option<Duration> {
            *self.0.lock().unwrap()
        }
    }

    impl ManualClock {
        fn set(&self, value: Option<Duration>) {
            *self.0.lock().unwrap() = value;
        }
    }

    #[test]
    fn denial_rate_limits_share_capacity_and_reject_clock_faults() {
        let clock = Arc::new(ManualClock(Mutex::new(Some(Duration::from_secs(100)))));
        let admission = Admission::new(
            Arc::new(ConfiguredPolicy::deny_all()),
            DenialLimits {
                max_records: 2,
                max_per_window: 2,
                window_seconds: 10,
            },
            clock.clone(),
        )
        .unwrap();
        let other = admission.clone();
        assert!(admission.denial.take_capacity());
        assert!(other.denial.take_capacity());
        assert!(!admission.denial.take_capacity());
        clock.set(Some(Duration::from_millis(109_999)));
        assert!(!other.denial.take_capacity());
        clock.set(Some(Duration::from_secs(99)));
        assert!(!admission.denial.take_capacity());
        clock.set(None);
        assert!(!other.denial.take_capacity());
        clock.set(Some(Duration::from_secs(110)));
        assert!(other.denial.take_capacity());
        assert!(admission.denial.take_capacity());
        assert!(!other.denial.take_capacity());
        clock.set(Some(Duration::from_millis(119_999)));
        assert!(!admission.denial.take_capacity());
        clock.set(Some(Duration::from_secs(120)));
        assert!(other.denial.take_capacity());
        assert_eq!(
            admission.diagnostics(),
            Diagnostics {
                rate_limited: 4,
                clock_unavailable: 2,
                ..Diagnostics::default()
            }
        );
        admission
            .denial
            .counts
            .storage_failed
            .store(u64::MAX - 1, Ordering::Relaxed);
        increment(&other.denial.counts.storage_failed);
        increment(&other.denial.counts.storage_failed);
        assert_eq!(admission.diagnostics().storage_failed, u64::MAX);
        assert_eq!(admission.diagnostics(), other.diagnostics());
    }
}
