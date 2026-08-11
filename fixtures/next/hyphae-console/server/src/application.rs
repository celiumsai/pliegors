// SPDX-License-Identifier: AGPL-3.0-only

mod state;

use crate::{ConsoleStore, ConsoleStoreError};
use futures_util::stream;
use pliego_dom::{IntoView, el};
use pliego_router::{RouteGraphBuilder, RouteMethod, RouteScopeKind, RouteScopeSpec, RouteSpec};
use pliego_runtime::{
    ActionNavigation, ActionPolicy, ActionRequestSecurity, ActionResponse, Body, CompleteDocument,
    CompleteRenderOptions, CsrfManager, CsrfToken, DataError, HandlerError, InMemorySessionStore,
    MiddlewareCapabilities, MiddlewareCapability, NativeRuntime, NativeRuntimeBuilder,
    OrderedDocument, OrderedRenderOptions, OrderedViewChunk, PublicErrorClass, RequestContext,
    Response, SecretHandle, SessionManager, SessionPolicy, SessionToken, StatusCode,
    action_failure_to_handler_error, decode_action_request, read_session_token,
    render_complete_document, render_ordered_document, session_cookie_header,
};
use serde::{Deserialize, Serialize};
use state::{ConsoleState, ConsoleStateStore, StateError, mutation_identity};
use std::fmt;
use std::sync::Arc;
use tokio::sync::Mutex;

type Sessions = SessionManager<InMemorySessionStore>;
const PREVIEW_PASSWORD: &str = "preview-only";

#[derive(Clone)]
struct Services {
    origin: Arc<str>,
    sessions: Sessions,
    csrf: CsrfManager,
    store: Arc<dyn ConsoleStateStore>,
    mutation: Arc<Mutex<()>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SessionClaims {
    subject: String,
    tenant_id: String,
    role: String,
    authenticated: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LoginInput {
    username: String,
    password: String,
    #[serde(rename = "_csrf")]
    csrf: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IncrementInput {
    expected_revision: u64,
    #[serde(rename = "_csrf")]
    csrf: String,
}

pub fn build_console_runtime(
    store: ConsoleStore,
    origin: &str,
    csrf_key: SecretHandle,
) -> Result<NativeRuntime, ConsoleApplicationError> {
    build_console_runtime_with_store(Arc::new(store), origin, csrf_key, None)
}

#[cfg(feature = "acceptance-harness")]
pub fn build_console_acceptance_runtime(
    store: ConsoleStore,
    origin: &str,
) -> Result<NativeRuntime, ConsoleApplicationError> {
    let mut key = vec![0_u8; 32];
    getrandom::fill(&mut key).map_err(|_| ConsoleApplicationError::Build)?;
    let cookie = pliego_runtime::SessionCookiePolicy::new("pliego-hyphae-acceptance-session")
        .map_err(|_| ConsoleApplicationError::Build)?
        .secure(false)
        .map_err(|_| ConsoleApplicationError::Build)?;
    build_console_runtime_with_store(
        Arc::new(store),
        origin,
        SecretHandle::new("console-csrf", 1, key).map_err(|_| ConsoleApplicationError::Build)?,
        Some(cookie),
    )
}

fn build_console_runtime_with_store(
    store: Arc<dyn ConsoleStateStore>,
    origin: &str,
    csrf_key: SecretHandle,
    cookie_policy: Option<pliego_runtime::SessionCookiePolicy>,
) -> Result<NativeRuntime, ConsoleApplicationError> {
    let origin = validate_origin(origin)?;
    let services = Services {
        origin,
        sessions: SessionManager::new(
            SessionPolicy::new("hyphae-console-session", 1)?
                .cookie(cookie_policy.unwrap_or_default()),
            InMemorySessionStore::new(),
        ),
        csrf: CsrfManager::new(csrf_key, []).map_err(|_| ConsoleApplicationError::Build)?,
        store,
        mutation: Arc::new(Mutex::new(())),
    };
    let login = ActionPolicy::new("login", 1, "login-input", "login-errors", "login-output")?
        .require_authentication(false)
        .require_authorization(false)
        .max_encoded_bytes(512)?
        .max_decoded_bytes(512)?
        .max_form_fields(3)?;
    let increment = ActionPolicy::new(
        "increment-console",
        1,
        "increment-input",
        "increment-errors",
        "increment-output",
    )?
    .max_encoded_bytes(512)?
    .max_decoded_bytes(512)?
    .max_form_fields(3)?;
    let graph = RouteGraphBuilder::new()
        .declare_middleware(
            "response-policy",
            MiddlewareCapabilities::none().allowing(MiddlewareCapability::MutateResponseHeaders),
        )?
        .error_boundary("root-error")?
        .scope(
            RouteScopeSpec::new("console-scope", RouteScopeKind::Group)?
                .middleware("response-policy")?,
        )
        .route(route("login-form", RouteMethod::get(), "/login")?)
        .route(route("login", RouteMethod::post(), "/login")?.action("login")?)
        .route(route("console", RouteMethod::get(), "/console")?)
        .route(
            route("increment", RouteMethod::post(), "/console/increment")?
                .action("increment-console")?,
        )
        .route(route("activity", RouteMethod::get(), "/console/activity")?)
        .seal()?;
    Ok(NativeRuntimeBuilder::new(graph, "hyphae-console-preview")?
        .action_policy(login)
        .action_policy(increment)
        .middleware_with_capabilities(
            "response-policy",
            MiddlewareCapabilities::none().allowing(MiddlewareCapability::MutateResponseHeaders),
            |_context, request, next: pliego_runtime::MiddlewareNext| async move {
                let mut response = next.run(request).await?;
                apply_response_policy(&mut response);
                Ok(response)
            },
        )
        .error_boundary(
            "root-error",
            |_context, error: pliego_runtime::PublicError| async move {
                let message = match error.class() {
                    PublicErrorClass::NotFound => {
                        "The requested route is not part of this Console."
                    }
                    PublicErrorClass::UnauthorizedOrForbidden => {
                        "This request cannot access the Console."
                    }
                    PublicErrorClass::InvalidRequest => "The Console rejected the request.",
                    PublicErrorClass::InternalFailure => "Console temporarily unavailable.",
                };
                let document = CompleteDocument::new(
                    "Console request failed",
                    el("main")
                        .child(el("h1").child("Request failed"))
                        .child(el("p").child(message))
                        .into_view(),
                );
                let mut response = render_complete_document(
                    &document,
                    CompleteRenderOptions::default().status(error.status()),
                )?;
                apply_response_policy(&mut response);
                Ok(response)
            },
        )
        .handler("login-form", {
            let services = services.clone();
            move |_context, _request| {
                let services = services.clone();
                async move { login_form(services).await }
            }
        })
        .handler("login", {
            let services = services.clone();
            move |context, request| {
                let services = services.clone();
                async move { login_action(context, request, services).await }
            }
        })
        .handler("console", {
            let services = services.clone();
            move |_context, request| {
                let services = services.clone();
                async move { console_page(request, services).await }
            }
        })
        .handler("increment", {
            let services = services.clone();
            move |context, request| {
                let services = services.clone();
                async move { increment_action(context, request, services).await }
            }
        })
        .handler("activity", move |_context, request| {
            let services = services.clone();
            async move { activity_page(request, services).await }
        })
        .build()?)
}

async fn login_form(services: Services) -> Result<Response<Body>, HandlerError> {
    let created = services
        .sessions
        .create(SessionClaims {
            subject: "anonymous".to_owned(),
            tenant_id: "anonymous".to_owned(),
            role: "anonymous".to_owned(),
            authenticated: false,
        })
        .await
        .map_err(internal)?;
    let csrf = services
        .csrf
        .issue(created.cookie.token(), "login", 1)
        .map_err(internal)?
        .as_form_value();
    let body = el("main")
        .child(el("h1").child("Reduced Hyphae Console"))
        .child(
            el("form")
                .attr("method", "post")
                .attr("action", "/login")
                .child(el("input").attr("name", "username"))
                .child(
                    el("input")
                        .attr("type", "password")
                        .attr("name", "password"),
                )
                .child(
                    el("input")
                        .attr("type", "hidden")
                        .attr("name", "_csrf")
                        .attr("value", csrf),
                )
                .child(el("button").attr("type", "submit").child("Sign in")),
        )
        .into_view();
    let mut response = render_complete_document(
        &CompleteDocument::new("Sign in", body),
        CompleteRenderOptions::default(),
    )?;
    let (name, value) = session_cookie_header(&created.cookie).map_err(internal)?;
    response.headers_mut().insert(name, value);
    Ok(response)
}

async fn login_action(
    context: RequestContext,
    request: pliego_runtime::Request<Body>,
    services: Services,
) -> Result<Response<Body>, HandlerError> {
    let token = required_token(&request, &services.sessions)?;
    let current = services
        .sessions
        .load::<SessionClaims>(&token)
        .await
        .map_err(internal)?
        .ok_or_else(unauthorized)?;
    if current.claims.authenticated {
        return Err(forbidden());
    }
    let policy = context
        .action_policy("login")
        .expect("login policy is sealed")
        .clone();
    let security = ActionRequestSecurity::new(services.origin.as_ref())?
        .authenticated(false)
        .authorized(false);
    let (input, admission) =
        decode_action_request::<LoginInput>(&context, &policy, request, &security).await?;
    let csrf_verified = verify_csrf(&services, &token, &input.csrf, "login")?;
    let admission = admission.csrf_verified(csrf_verified);
    let cookie = Arc::new(std::sync::Mutex::new(None));
    let cookie_output = cookie.clone();
    let sessions = services.sessions.clone();
    let old_token = token.clone();
    let mutation = move |action: pliego_runtime::ActionContext, input: LoginInput| {
        let sessions = sessions.clone();
        let old_token = old_token.clone();
        let cookie_output = cookie_output.clone();
        async move {
            let Some((subject, tenant_id)) = credential_identity(&input.username, &input.password)
            else {
                return Ok(ActionResponse::Invalid {
                    field_errors: vec!["The preview credentials are invalid.".to_owned()],
                });
            };
            action.commit().begin_commit()?;
            let rotated = sessions
                .rotate(
                    &old_token,
                    SessionClaims {
                        subject: subject.to_owned(),
                        tenant_id: tenant_id.to_owned(),
                        role: "member".to_owned(),
                        authenticated: true,
                    },
                )
                .await
                .map_err(|_| DataError::ActionOutcomeUnknown)?
                .ok_or(DataError::ActionOutcomeUnknown)?;
            action.commit().committed()?;
            *cookie_output
                .lock()
                .map_err(|_| DataError::ActionOutcomeUnknown)? = Some(rotated.cookie);
            Ok(ActionResponse::Success {
                output: subject.to_owned(),
                navigation: ActionNavigation::SeeOther("/console".to_owned()),
            })
        }
    };
    let result = context
        .data()
        .act(&policy, &admission, &mutation, input)
        .await
        .map_err(action_failure_to_handler_error)?;
    match result {
        ActionResponse::Invalid { .. } => form_error("The preview credentials are invalid."),
        ActionResponse::Success { .. } => {
            let cookie = cookie
                .lock()
                .map_err(|_| HandlerError::internal("session cookie lock failed"))?
                .take()
                .ok_or_else(|| HandlerError::internal("rotated session cookie is missing"))?;
            let mut response = redirect("/console")?;
            let (name, value) = session_cookie_header(&cookie).map_err(internal)?;
            response.headers_mut().insert(name, value);
            Ok(response)
        }
    }
}

async fn console_page(
    request: pliego_runtime::Request<Body>,
    services: Services,
) -> Result<Response<Body>, HandlerError> {
    let token = required_token(&request, &services.sessions)?;
    drop(request);
    let claims = authenticated_claims(&token, &services.sessions).await?;
    let state = load_state(&services, &claims.tenant_id).await?;
    let csrf = services
        .csrf
        .issue(&token, "increment-console", 1)
        .map_err(internal)?
        .as_form_value();
    let body = el("main")
        .attr("data-user", &claims.subject)
        .attr("data-tenant", &claims.tenant_id)
        .child(el("h1").child("Tenant console"))
        .child(
            el("p")
                .attr("data-counter", state.counter().to_string())
                .child(format!("Counter {}", state.counter())),
        )
        .child(
            el("p")
                .attr("data-revision", state.revision().to_string())
                .child(format!("Revision {}", state.revision())),
        )
        .child(
            el("form")
                .attr("method", "post")
                .attr("action", "/console/increment")
                .child(
                    el("input")
                        .attr("type", "hidden")
                        .attr("name", "expected_revision")
                        .attr("value", state.revision().to_string()),
                )
                .child(
                    el("input")
                        .attr("type", "hidden")
                        .attr("name", "_csrf")
                        .attr("value", csrf),
                )
                .child(el("button").attr("type", "submit").child("Increment")),
        )
        .child(el("a").attr("href", "/console/activity").child("Activity"))
        .into_view();
    render_complete_document(
        &CompleteDocument::new("Tenant console", body),
        CompleteRenderOptions::default(),
    )
}

async fn increment_action(
    context: RequestContext,
    request: pliego_runtime::Request<Body>,
    services: Services,
) -> Result<Response<Body>, HandlerError> {
    let token = required_token(&request, &services.sessions)?;
    let claims = authenticated_claims(&token, &services.sessions).await?;
    let policy = context
        .action_policy("increment-console")
        .expect("increment policy is sealed")
        .clone();
    let security = ActionRequestSecurity::new(services.origin.as_ref())?
        .authenticated(true)
        .authorized(claims.role == "member");
    let (input, admission) =
        decode_action_request::<IncrementInput>(&context, &policy, request, &security).await?;
    let csrf_verified = verify_csrf(&services, &token, &input.csrf, "increment-console")?;
    let admission = admission.csrf_verified(csrf_verified);
    let operation_services = services.clone();
    let mutation = move |action: pliego_runtime::ActionContext, input: IncrementInput| {
        let services = operation_services.clone();
        let subject = claims.subject.clone();
        let tenant_id = claims.tenant_id.clone();
        async move {
            let _guard = services.mutation.lock().await;
            let mut state = load_state_data(&services, &tenant_id).await?;
            if state.revision() != input.expected_revision {
                return Ok(ActionResponse::Invalid {
                    field_errors: vec!["The Console changed. Refresh before retrying.".to_owned()],
                });
            }
            state
                .increment(&subject)
                .map_err(|_| DataError::ActionFailure("console-state-invalid".to_owned()))?;
            let encoded = state
                .encode()
                .map_err(|_| DataError::ActionFailure("console-state-invalid".to_owned()))?;
            let identity = mutation_identity(&tenant_id, &encoded);
            action.commit().begin_commit()?;
            match services.store.put(&tenant_id, &encoded, identity).await {
                Ok(()) => {}
                Err(ConsoleStoreError::MutationRolledBack) => {
                    action.commit().failed()?;
                    return Err(DataError::ActionFailure(
                        "console-mutation-rolled-back".to_owned(),
                    ));
                }
                Err(_) => {
                    action.commit().outcome_unknown()?;
                    return Err(DataError::ActionOutcomeUnknown);
                }
            }
            action.commit().committed()?;
            Ok(ActionResponse::Success {
                output: state.revision(),
                navigation: ActionNavigation::SeeOther("/console".to_owned()),
            })
        }
    };
    match context
        .data()
        .act(&policy, &admission, &mutation, input)
        .await
        .map_err(|failure| {
            if failure.error()
                == &DataError::ActionFailure("console-mutation-rolled-back".to_owned())
            {
                HandlerError::new(
                    StatusCode::CONFLICT,
                    pliego_runtime::RuntimeDiagnostic::new(
                        "PLG-CON-409",
                        "Console mutation rolled back; refresh before retrying",
                    )
                    .expect("static diagnostic is valid"),
                )
            } else {
                action_failure_to_handler_error(failure)
            }
        })? {
        ActionResponse::Invalid { field_errors } => form_error(
            field_errors
                .first()
                .map_or("The Console action was rejected.", String::as_str),
        ),
        ActionResponse::Success { .. } => redirect("/console"),
    }
}

async fn activity_page(
    request: pliego_runtime::Request<Body>,
    services: Services,
) -> Result<Response<Body>, HandlerError> {
    let token = required_token(&request, &services.sessions)?;
    drop(request);
    let claims = authenticated_claims(&token, &services.sessions).await?;
    let state = load_state(&services, &claims.tenant_id).await?;
    let mut chunks = Vec::with_capacity(state.activity().len() + 2);
    chunks.push(OrderedViewChunk::new({
        let tenant = claims.tenant_id.clone();
        let counter = state.counter();
        let revision = state.revision();
        move || {
            el("main")
                .attr("data-tenant", tenant)
                .child(el("h1").child("Tenant activity"))
                .child(el("p").child(format!("Counter {counter}, revision {revision}")))
                .into_view()
        }
    }));
    for item in state.activity() {
        let item = item.clone();
        chunks.push(OrderedViewChunk::new(move || {
            el("article")
                .attr("data-revision", item.revision().to_string())
                .child(format!("{} by {}", item.kind(), item.actor()))
                .into_view()
        }));
    }
    chunks.push(OrderedViewChunk::new(|| {
        el("nav")
            .child(el("a").attr("href", "/console").child("Return to console"))
            .into_view()
    }));
    render_ordered_document(
        &OrderedDocument::new("Tenant activity"),
        stream::iter(chunks),
        OrderedRenderOptions::default(),
    )
}

async fn authenticated_claims(
    token: &SessionToken,
    sessions: &Sessions,
) -> Result<SessionClaims, HandlerError> {
    let session = sessions
        .load::<SessionClaims>(token)
        .await
        .map_err(internal)?
        .ok_or_else(unauthorized)?;
    if !session.claims.authenticated || session.claims.role != "member" {
        return Err(unauthorized());
    }
    Ok(session.claims)
}

async fn load_state(services: &Services, tenant_id: &str) -> Result<ConsoleState, HandlerError> {
    match services.store.get(tenant_id).await.map_err(store_error)? {
        Some(bytes) => ConsoleState::decode(tenant_id, &bytes).map_err(state_error),
        None => ConsoleState::empty(tenant_id).map_err(state_error),
    }
}

async fn load_state_data(services: &Services, tenant_id: &str) -> Result<ConsoleState, DataError> {
    match services
        .store
        .get(tenant_id)
        .await
        .map_err(|_| DataError::ActionFailure("console-store-unavailable".to_owned()))?
    {
        Some(bytes) => ConsoleState::decode(tenant_id, &bytes)
            .map_err(|_| DataError::ActionFailure("console-state-invalid".to_owned())),
        None => ConsoleState::empty(tenant_id)
            .map_err(|_| DataError::ActionFailure("console-state-invalid".to_owned())),
    }
}

fn required_token(
    request: &pliego_runtime::Request<Body>,
    sessions: &Sessions,
) -> Result<SessionToken, HandlerError> {
    read_session_token(request.headers(), sessions.policy().cookie_policy())
        .map_err(internal)?
        .ok_or_else(unauthorized)
}

fn credential_identity(username: &str, password: &str) -> Option<(&'static str, &'static str)> {
    if password != PREVIEW_PASSWORD {
        return None;
    }
    match username {
        "alice" => Some(("alice", "tenant-a")),
        "bob" => Some(("bob", "tenant-b")),
        _ => None,
    }
}

fn verify_csrf(
    services: &Services,
    session: &SessionToken,
    authored: &str,
    action: &str,
) -> Result<bool, HandlerError> {
    let token = CsrfToken::parse(authored).map_err(|_| forbidden())?;
    let verified = services
        .csrf
        .verify(&token, session, action, 1)
        .map_err(|_| forbidden())?;
    Ok(verified)
}

fn route(
    id: &str,
    method: RouteMethod,
    path: &str,
) -> Result<RouteSpec, pliego_router::RouteError> {
    RouteSpec::new(id, method, path)?.scope("console-scope")
}

fn validate_origin(origin: &str) -> Result<Arc<str>, ConsoleApplicationError> {
    if !(origin.starts_with("http://") || origin.starts_with("https://"))
        || origin.ends_with('/')
        || origin.contains(['?', '#'])
    {
        return Err(ConsoleApplicationError::Origin);
    }
    Ok(Arc::from(origin))
}

fn apply_response_policy(response: &mut Response<Body>) {
    for (name, value) in [
        ("cache-control", "no-store"),
        (
            "content-security-policy",
            "default-src 'none'; connect-src 'none'; style-src 'none'; form-action 'self'; base-uri 'none'; frame-ancestors 'none'",
        ),
        ("referrer-policy", "no-referrer"),
        ("x-content-type-options", "nosniff"),
        ("cross-origin-resource-policy", "same-origin"),
    ] {
        response.headers_mut().insert(
            http::HeaderName::from_static(name),
            http::HeaderValue::from_static(value),
        );
    }
}

fn redirect(location: &str) -> Result<Response<Body>, HandlerError> {
    Response::builder()
        .status(StatusCode::SEE_OTHER)
        .header(http::header::LOCATION, location)
        .body(Body::empty())
        .map_err(|_| HandlerError::internal("redirect response could not be built"))
}

fn form_error(message: &str) -> Result<Response<Body>, HandlerError> {
    let document = CompleteDocument::new(
        "Console action rejected",
        el("main")
            .child(el("h1").child("Action rejected"))
            .child(el("p").attr("role", "alert").child(message))
            .into_view(),
    );
    render_complete_document(
        &document,
        CompleteRenderOptions::default().status(StatusCode::UNPROCESSABLE_ENTITY),
    )
}

fn unauthorized() -> HandlerError {
    HandlerError::new(
        StatusCode::UNAUTHORIZED,
        pliego_runtime::RuntimeDiagnostic::new("PLG-CON-401", "Console session is required")
            .expect("static diagnostic is valid"),
    )
}

fn forbidden() -> HandlerError {
    HandlerError::new(
        StatusCode::FORBIDDEN,
        pliego_runtime::RuntimeDiagnostic::new("PLG-CON-403", "Console action is forbidden")
            .expect("static diagnostic is valid"),
    )
}

fn internal(error: impl fmt::Display) -> HandlerError {
    HandlerError::internal(error.to_string())
}

fn state_error(_: StateError) -> HandlerError {
    HandlerError::internal("Console state failed validation")
}

fn store_error(error: ConsoleStoreError) -> HandlerError {
    let status = match error {
        ConsoleStoreError::MutationRolledBack => StatusCode::CONFLICT,
        ConsoleStoreError::OutcomeUnknown => StatusCode::CONFLICT,
        _ => StatusCode::BAD_GATEWAY,
    };
    HandlerError::new(
        status,
        pliego_runtime::RuntimeDiagnostic::new("PLG-CON-502", "Console store is unavailable")
            .expect("static diagnostic is valid"),
    )
}

#[derive(Debug)]
pub enum ConsoleApplicationError {
    Build,
    Origin,
}

impl fmt::Display for ConsoleApplicationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Build => "Console runtime could not be built",
            Self::Origin => "Console origin is invalid",
        })
    }
}

impl std::error::Error for ConsoleApplicationError {}

macro_rules! build_error_conversion {
    ($($error:ty),+ $(,)?) => {
        $(impl From<$error> for ConsoleApplicationError {
            fn from(_: $error) -> Self { Self::Build }
        })+
    };
}

build_error_conversion!(
    pliego_router::RouteError,
    pliego_runtime::DataError,
    pliego_runtime::SessionError,
    pliego_runtime::RuntimeBuildError,
);

#[cfg(test)]
mod tests;
