// SPDX-License-Identifier: Apache-2.0

use pliego_router::{RouteGraphBuilder, RouteMethod, RouteResourceSpec, RouteSpec};
use pliego_runtime::{
    ActionIdempotency, ActionInvalidationIntent, ActionNavigation, ActionPolicy,
    ActionRequestSecurity, ActionResponse, Body, CacheDomain, CacheKey, CacheKeyInput,
    CacheManager, CachePartition, CachePolicy, CacheTag, CapabilitySet, CsrfManager, DataError,
    HandlerError, IdempotencyKey, IdempotencyManager, IdempotencyPartition, IdempotencyPolicy,
    InMemoryCacheStore, InMemoryIdempotencyStore, InMemoryInvalidationCoordinator,
    InMemoryReceiptSink, InMemorySessionStore, LoaderPolicy, NativeRuntime, NativeRuntimeBuilder,
    RequestContext, ResourceRegistryBuilder, ResourceRequirement, ResourceSpec, Response,
    RuntimeDiagnostic, SecretHandle, SessionCsrfContext, SessionManager, SessionPolicy,
    SessionToken, StatusCode, action_failure_to_handler_error, decode_session_action_request,
    read_session_token, session_cookie_header,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::error::Error;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

type AppResult<T> = Result<T, Box<dyn Error + Send + Sync>>;
type Sessions = SessionManager<InMemorySessionStore>;
type Idempotency = IdempotencyManager<InMemoryIdempotencyStore>;
type MemoryCache = CacheManager<InMemoryCacheStore>;
type FieldErrors = BTreeMap<String, String>;

const ORIGIN: &str = "https://example.com";
const PREVIEW_PASSWORD: &str = "preview-only";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SessionClaims {
    user_id: String,
    authenticated: bool,
    role: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LoginInput {
    username: String,
    password: String,
    #[serde(rename = "_csrf")]
    csrf: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct LoginOutput {
    user_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RenameInput {
    display_name: String,
    idempotency_key: String,
    #[serde(rename = "_csrf")]
    csrf: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct RenameOutput {
    display_name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct AccountQuery {
    user_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct AccountView {
    display_name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CatalogQuery {
    revision: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CatalogView {
    names: Vec<String>,
}

#[derive(Default)]
struct AccountsState {
    display_names: BTreeMap<String, String>,
    mutation_count: usize,
}

#[derive(Clone, Default)]
struct AccountStore {
    state: Arc<Mutex<AccountsState>>,
}

impl AccountStore {
    fn ensure_user(&self, user_id: &str) {
        lock(&self.state)
            .display_names
            .entry(user_id.to_owned())
            .or_insert_with(|| user_id.to_owned());
    }

    fn account(&self, user_id: &str) -> AccountView {
        AccountView {
            display_name: lock(&self.state)
                .display_names
                .get(user_id)
                .cloned()
                .unwrap_or_else(|| user_id.to_owned()),
        }
    }

    fn find_account(&self, user_id: &str) -> Option<AccountView> {
        lock(&self.state)
            .display_names
            .get(user_id)
            .cloned()
            .map(|display_name| AccountView { display_name })
    }

    fn catalog(&self) -> CatalogView {
        CatalogView {
            names: lock(&self.state).display_names.values().cloned().collect(),
        }
    }

    fn rename(&self, user_id: &str, display_name: &str) {
        let mut state = lock(&self.state);
        state
            .display_names
            .insert(user_id.to_owned(), display_name.to_owned());
        state.mutation_count += 1;
    }

    fn mutation_count(&self) -> usize {
        lock(&self.state).mutation_count
    }
}

#[derive(Clone)]
struct SharedServices {
    origin: Arc<str>,
    sessions: Sessions,
    idempotency: Idempotency,
    csrf: CsrfManager,
    accounts: AccountStore,
    public_policy: CachePolicy,
    private_policy: CachePolicy,
    public_invalidation: InMemoryInvalidationCoordinator,
    private_invalidation: InMemoryInvalidationCoordinator,
}

pub struct FullstackCluster {
    pub first: NativeRuntime,
    pub second: NativeRuntime,
    pub first_receipts: InMemoryReceiptSink,
    pub second_receipts: InMemoryReceiptSink,
    accounts: AccountStore,
    sessions: Sessions,
}

impl FullstackCluster {
    pub fn mutation_count(&self) -> usize {
        self.accounts.mutation_count()
    }

    pub async fn revoke_cookie(&self, cookie_header: &str) -> AppResult<bool> {
        let headers = http::HeaderMap::from_iter([(
            http::header::COOKIE,
            http::HeaderValue::from_str(cookie_header)?,
        )]);
        let token = read_session_token(&headers, self.sessions.policy().cookie_policy())?
            .ok_or("session cookie is missing")?;
        Ok(self.sessions.revoke(&token).await?)
    }

    pub fn write_contract_manifest(&self, path: &Path) -> AppResult<()> {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        let bytes = serde_json::to_vec_pretty(&self.first.contract_manifest())?;
        std::fs::write(path, bytes)?;
        Ok(())
    }
}

pub fn build_cluster() -> AppResult<FullstackCluster> {
    build_cluster_for_origin(ORIGIN)
}

pub fn build_cluster_for_origin(origin: &str) -> AppResult<FullstackCluster> {
    let origin = url::Url::parse(origin)?;
    if !matches!(origin.scheme(), "http" | "https")
        || origin.host_str().is_none()
        || origin.path() != "/"
        || origin.query().is_some()
        || origin.fragment().is_some()
    {
        return Err("reference Origin must be an absolute HTTP origin".into());
    }
    let origin: Arc<str> = Arc::from(origin.origin().ascii_serialization());
    let accounts = AccountStore::default();
    let sessions = SessionManager::new(
        SessionPolicy::new("application-session", 1)?,
        InMemorySessionStore::new(),
    );
    let idempotency_policy = IdempotencyPolicy::new("account-mutations")?;
    let idempotency = IdempotencyManager::new(idempotency_policy, InMemoryIdempotencyStore::new());
    let csrf = CsrfManager::new(
        SecretHandle::new(
            "csrf-signing",
            1,
            b"pliegors-g2-reference-csrf-key-0001".to_vec(),
        )?,
        [],
    )?;
    let public_policy = CachePolicy::new(
        "catalog-public",
        1,
        "catalog",
        1,
        CacheDomain::PublicRuntime,
    )?;
    let private_policy = CachePolicy::new(
        "account-private",
        1,
        "accounts",
        1,
        CacheDomain::PrivateSession,
    )?;
    let public_invalidation = InMemoryInvalidationCoordinator::new();
    let private_invalidation = InMemoryInvalidationCoordinator::new();
    let first_public_store = InMemoryCacheStore::new(128)?;
    let second_public_store = InMemoryCacheStore::new(128)?;
    let first_private_store = InMemoryCacheStore::new(128)?;
    let second_private_store = InMemoryCacheStore::new(128)?;
    public_invalidation.register(&first_public_store);
    public_invalidation.register(&second_public_store);
    private_invalidation.register(&first_private_store);
    private_invalidation.register(&second_private_store);

    let shared = SharedServices {
        origin,
        sessions: sessions.clone(),
        idempotency,
        csrf,
        accounts: accounts.clone(),
        public_policy: public_policy.clone(),
        private_policy: private_policy.clone(),
        public_invalidation,
        private_invalidation,
    };
    let first_receipts = InMemoryReceiptSink::default();
    let second_receipts = InMemoryReceiptSink::default();
    let first = build_runtime(
        "g2-replica-a",
        shared.clone(),
        CacheManager::new(public_policy.clone(), first_public_store),
        CacheManager::new(private_policy.clone(), first_private_store),
        first_receipts.clone(),
    )?;
    let second = build_runtime(
        "g2-replica-b",
        shared,
        CacheManager::new(public_policy, second_public_store),
        CacheManager::new(private_policy, second_private_store),
        second_receipts.clone(),
    )?;
    Ok(FullstackCluster {
        first,
        second,
        first_receipts,
        second_receipts,
        accounts,
        sessions,
    })
}

fn build_runtime(
    deployment_id: &str,
    shared: SharedServices,
    public_cache: MemoryCache,
    private_cache: MemoryCache,
    receipts: InMemoryReceiptSink,
) -> AppResult<NativeRuntime> {
    let read = ResourceRequirement::new("accounts")?.requiring("read")?;
    let write = ResourceRequirement::new("accounts")?.requiring("write")?;
    let account_loader = LoaderPolicy::new("account-loader", 1, "account-query", "account-view")?
        .resource(read.clone())?
        .cache_policy("account-private")?;
    let catalog_loader = LoaderPolicy::new("catalog-loader", 1, "catalog-query", "catalog-view")?
        .resource(read.clone())?
        .cache_policy("catalog-public")?;
    let public_account_loader =
        LoaderPolicy::new("public-account-loader", 1, "account-query", "account-view")?
            .resource(read)?;
    let login_policy =
        ActionPolicy::new("login", 1, "login-input", "login-errors", "login-output")?
            .require_authentication(false)
            .require_authorization(false)
            .resource(write.clone())?;
    let rename_policy = ActionPolicy::new(
        "rename-account",
        1,
        "rename-input",
        "rename-errors",
        "rename-output",
    )?
    .resource(write)?
    .idempotency(shared.idempotency.policy())
    .invalidation(
        ActionInvalidationIntent::tags("catalog-public", [CacheTag::new("account-public")?])?
            .read_your_writes(),
    )?
    .invalidation(
        ActionInvalidationIntent::tags("account-private", [CacheTag::new("account-private")?])?
            .read_your_writes(),
    )?;

    let graph = RouteGraphBuilder::new()
        .route(RouteSpec::new(
            "favicon",
            RouteMethod::get(),
            "/favicon.svg",
        )?)
        .route(RouteSpec::new("login-form", RouteMethod::get(), "/login")?)
        .route(
            RouteSpec::new("login", RouteMethod::post(), "/login")?
                .action("login")?
                .resource(RouteResourceSpec::new("accounts")?.requiring("write")?)?,
        )
        .route(
            RouteSpec::new("dashboard", RouteMethod::get(), "/dashboard")?
                .loader("account-loader")?
                .cache_policy("account-private")?
                .resource(RouteResourceSpec::new("accounts")?.requiring("read")?)?,
        )
        .route(
            RouteSpec::new("rename", RouteMethod::post(), "/account/rename")?
                .action("rename-account")?
                .resource(RouteResourceSpec::new("accounts")?.requiring("write")?)?,
        )
        .route(
            RouteSpec::new("catalog", RouteMethod::get(), "/catalog")?
                .loader("catalog-loader")?
                .cache_policy("catalog-public")?
                .resource(RouteResourceSpec::new("accounts")?.requiring("read")?)?,
        )
        .route(
            RouteSpec::new("account-profile", RouteMethod::get(), "/accounts/:user_id")?
                .loader("public-account-loader")?
                .resource(RouteResourceSpec::new("accounts")?.requiring("read")?)?,
        )
        .seal()?;

    let capabilities = CapabilitySet::none().allowing("read")?.allowing("write")?;
    let resources = ResourceRegistryBuilder::new()
        .register(
            ResourceSpec::new("accounts", "reference-memory")?.with_capabilities(capabilities),
            shared.accounts.clone(),
        )?
        .seal();

    Ok(NativeRuntimeBuilder::new(graph, deployment_id)?
        .resources(resources)
        .loader_policy(account_loader)
        .loader_policy(catalog_loader)
        .loader_policy(public_account_loader)
        .action_policy(login_policy)
        .action_policy(rename_policy)
        .cache_policy(shared.private_policy.clone())
        .cache_policy(shared.public_policy.clone())
        .handler("favicon", |_context: RequestContext, _request| async {
            favicon()
        })
        .handler("login-form", {
            let shared = shared.clone();
            move |_context: RequestContext, _request| {
                let shared = shared.clone();
                async move { login_form(shared).await }
            }
        })
        .handler("login", {
            let shared = shared.clone();
            move |context: RequestContext, request| {
                let shared = shared.clone();
                async move { login_action(context, request, shared).await }
            }
        })
        .handler("dashboard", {
            let shared = shared.clone();
            let private_cache = private_cache.clone();
            move |context: RequestContext, request| {
                let shared = shared.clone();
                let private_cache = private_cache.clone();
                async move { dashboard(context, request, shared, private_cache).await }
            }
        })
        .handler("rename", {
            let shared = shared.clone();
            move |context: RequestContext, request| {
                let shared = shared.clone();
                async move { rename_action(context, request, shared).await }
            }
        })
        .handler("catalog", {
            let public_cache = public_cache.clone();
            move |context: RequestContext, _request| {
                let public_cache = public_cache.clone();
                async move { catalog(context, public_cache).await }
            }
        })
        .handler(
            "account-profile",
            |context: RequestContext, _request| async move { account_profile(context).await },
        )
        .receipt_sink(receipts)
        .build()?)
}

async fn login_form(shared: SharedServices) -> Result<Response<Body>, HandlerError> {
    let created = shared
        .sessions
        .create(SessionClaims {
            user_id: "anonymous".to_owned(),
            authenticated: false,
            role: "anonymous".to_owned(),
        })
        .await
        .map_err(internal)?;
    let csrf = shared
        .csrf
        .issue(created.cookie.token(), "login", 1)
        .map_err(internal)?
        .as_form_value();
    let mut response = html_response(
        StatusCode::OK,
        &page(
            "Sign in",
            &format!(
                "<h1>PliegoRS G2</h1><form method=\"post\" action=\"/login\"><label>Username<input name=\"username\" autocomplete=\"username\"></label><label>Password<input type=\"password\" name=\"password\" autocomplete=\"current-password\"></label><input type=\"hidden\" name=\"_csrf\" value=\"{}\"><button type=\"submit\">Sign in</button></form>",
                escape(&csrf)
            ),
        ),
    )?;
    let (name, value) = session_cookie_header(&created.cookie).map_err(internal)?;
    response.headers_mut().insert(name, value);
    Ok(response)
}

async fn login_action(
    context: RequestContext,
    request: pliego_runtime::Request<Body>,
    shared: SharedServices,
) -> Result<Response<Body>, HandlerError> {
    let token = required_token(&request, &shared.sessions)?;
    let current = shared
        .sessions
        .load::<SessionClaims>(&token)
        .await
        .map_err(internal)?
        .ok_or_else(unauthorized)?;
    if current.claims.authenticated {
        return Err(forbidden(
            "PLG-AUTH-403",
            "session is already authenticated",
        ));
    }
    let policy = context
        .action_policy("login")
        .expect("login policy is sealed")
        .clone();
    let security = ActionRequestSecurity::new(shared.origin.as_ref())?
        .authenticated(false)
        .authorized(false);
    let (input, admission) = decode_session_action_request::<LoginInput, _>(
        &context,
        &policy,
        request,
        &security,
        SessionCsrfContext::new(&shared.csrf, &token),
        |input| Some(input.csrf.clone()),
    )
    .await?;
    let cookie = Arc::new(Mutex::new(None));
    let cookie_output = cookie.clone();
    let sessions = shared.sessions.clone();
    let old_token = token.clone();
    let mutation = move |action: pliego_runtime::ActionContext, input: LoginInput| {
        let sessions = sessions.clone();
        let old_token = old_token.clone();
        let cookie_output = cookie_output.clone();
        async move {
            if !valid_user_id(&input.username) || input.password != PREVIEW_PASSWORD {
                return Ok(ActionResponse::Invalid {
                    field_errors: BTreeMap::from([(
                        "credentials".to_owned(),
                        "The preview credentials are invalid.".to_owned(),
                    )]),
                });
            }
            let accounts = action.resource::<AccountStore>("accounts")?;
            action.commit().begin_commit()?;
            accounts.use_with(|store| store.ensure_user(&input.username))?;
            let claims = SessionClaims {
                user_id: input.username.clone(),
                authenticated: true,
                role: "member".to_owned(),
            };
            let rotated = match sessions.rotate(&old_token, claims).await {
                Ok(Some(rotated)) => rotated,
                Ok(None) | Err(_) => {
                    action.commit().outcome_unknown()?;
                    return Err(DataError::ActionOutcomeUnknown);
                }
            };
            action.commit().committed()?;
            *lock(&cookie_output) = Some(rotated.cookie);
            Ok(ActionResponse::Success {
                output: LoginOutput {
                    user_id: input.username,
                },
                navigation: ActionNavigation::SeeOther("/dashboard".to_owned()),
            })
        }
    };
    let result = context
        .data()
        .act(&policy, &admission, &mutation, input)
        .await
        .map_err(action_failure_to_handler_error)?;
    match result {
        ActionResponse::Invalid { field_errors } => {
            typed_form_error("Sign in failed", "/login", &field_errors)
        }
        ActionResponse::Success { .. } => {
            let cookie = lock(&cookie)
                .take()
                .ok_or_else(|| HandlerError::internal("rotated session cookie is missing"))?;
            let (name, value) = session_cookie_header(&cookie).map_err(internal)?;
            let mut response = redirect("/dashboard")?;
            response.headers_mut().insert(name, value);
            Ok(response)
        }
    }
}

async fn dashboard(
    context: RequestContext,
    request: pliego_runtime::Request<Body>,
    shared: SharedServices,
    cache: MemoryCache,
) -> Result<Response<Body>, HandlerError> {
    let token = required_token(&request, &shared.sessions)?;
    let session = authenticated_session(&shared.sessions, &token).await?;
    let cache_policy = context
        .cache_policy("account-private")
        .expect("private cache policy is sealed");
    let key = private_key(cache_policy, &session.claims.user_id, &token).map_err(internal)?;
    let account = match context
        .data()
        .cache_lookup::<_, AccountView>(&cache, &key)
        .await
        .map_err(internal)?
        .value
    {
        Some(value) => value,
        None => {
            let loader_policy = context
                .loader_policy("account-loader")
                .expect("account loader is sealed")
                .clone();
            let loader = |loader: pliego_runtime::LoaderContext, query: AccountQuery| async move {
                loader
                    .resource::<AccountStore>("accounts")?
                    .use_with(|store| store.account(&query.user_id))
            };
            let loaded = context
                .data()
                .load(
                    &loader_policy,
                    &loader,
                    AccountQuery {
                        user_id: session.claims.user_id.clone(),
                    },
                )
                .await
                .map_err(internal)?;
            let value = (*loaded).clone();
            context
                .data()
                .cache_insert(
                    &cache,
                    &key,
                    &value,
                    [CacheTag::new("account-private").map_err(internal)?],
                )
                .await
                .map_err(internal)?;
            value
        }
    };
    let csrf = shared
        .csrf
        .issue(&token, "rename-account", 1)
        .map_err(internal)?
        .as_form_value();
    html_response(
        StatusCode::OK,
        &page(
            "Dashboard",
            &format!(
                "<p>Signed in as <strong>{}</strong></p><h1>{}</h1><form method=\"post\" action=\"/account/rename\"><label>Display name<input name=\"display_name\" value=\"{}\"></label><input type=\"hidden\" name=\"idempotency_key\" value=\"rename-request-0001\"><input type=\"hidden\" name=\"_csrf\" value=\"{}\"><button type=\"submit\">Save</button></form><p><a href=\"/catalog\">Public catalog</a></p>",
                escape(&session.claims.user_id),
                escape(&account.display_name),
                escape(&account.display_name),
                escape(&csrf)
            ),
        ),
    )
}

async fn rename_action(
    context: RequestContext,
    request: pliego_runtime::Request<Body>,
    shared: SharedServices,
) -> Result<Response<Body>, HandlerError> {
    let token = required_token(&request, &shared.sessions)?;
    let session = authenticated_session(&shared.sessions, &token).await?;
    let policy = context
        .action_policy("rename-account")
        .expect("rename policy is sealed")
        .clone();
    let security = ActionRequestSecurity::new(shared.origin.as_ref())?
        .authenticated(true)
        .authorized(session.claims.role == "member");
    let (input, admission) = decode_session_action_request::<RenameInput, _>(
        &context,
        &policy,
        request,
        &security,
        SessionCsrfContext::new(&shared.csrf, &token),
        |input| Some(input.csrf.clone()),
    )
    .await?;
    let key = IdempotencyKey::parse(input.idempotency_key.clone()).map_err(internal)?;
    let partition =
        IdempotencyPartition::from_identity(&session.claims.user_id).map_err(internal)?;
    let user_id = session.claims.user_id.clone();
    let public_policy = shared.public_policy.clone();
    let private_policy = shared.private_policy.clone();
    let public_invalidation = shared.public_invalidation.clone();
    let private_invalidation = shared.private_invalidation.clone();
    let cause_receipt = context.scope().identity().request_id.clone();
    let mutation = move |action: pliego_runtime::ActionContext, input: RenameInput| {
        let user_id = user_id.clone();
        let public_policy = public_policy.clone();
        let private_policy = private_policy.clone();
        let public_invalidation = public_invalidation.clone();
        let private_invalidation = private_invalidation.clone();
        let cause_receipt = cause_receipt.clone();
        async move {
            let normalized = input.display_name.trim().to_owned();
            if normalized.is_empty() || normalized.len() > 80 {
                return Ok(ActionResponse::Invalid {
                    field_errors: BTreeMap::from([(
                        "display_name".to_owned(),
                        "Display name must contain between 1 and 80 bytes.".to_owned(),
                    )]),
                });
            }
            let accounts = action.resource::<AccountStore>("accounts")?;
            action.commit().begin_commit()?;
            accounts.use_with(|store| store.rename(&user_id, &normalized))?;
            action.commit().committed()?;
            let public_event = public_invalidation
                .invalidate_tags(
                    &public_policy,
                    [CacheTag::new("account-public")
                        .map_err(|error| DataError::ActionFailure(error.to_string()))?],
                    cause_receipt.clone(),
                )
                .map_err(|error| DataError::ActionFailure(error.to_string()))?;
            let private_event = private_invalidation
                .invalidate_tags(
                    &private_policy,
                    [CacheTag::new("account-private")
                        .map_err(|error| DataError::ActionFailure(error.to_string()))?],
                    cause_receipt,
                )
                .map_err(|error| DataError::ActionFailure(error.to_string()))?;
            if let Err(error) = action.record_invalidation(public_event) {
                action.commit().compensation_required()?;
                return Err(error);
            }
            if let Err(error) = action.record_invalidation(private_event) {
                action.commit().compensation_required()?;
                return Err(error);
            }
            Ok(ActionResponse::Success {
                output: RenameOutput {
                    display_name: normalized,
                },
                navigation: ActionNavigation::SeeOther("/dashboard".to_owned()),
            })
        }
    };
    let result = context
        .data()
        .act_idempotent(
            &policy,
            &admission,
            &mutation,
            input,
            ActionIdempotency::new(&shared.idempotency, &key, &partition, 1).map_err(internal)?,
        )
        .await
        .map_err(action_failure_to_handler_error)?;
    match result {
        ActionResponse::Success { .. } => redirect("/dashboard"),
        ActionResponse::Invalid { field_errors } => {
            typed_form_error("Account update failed", "/dashboard", &field_errors)
        }
    }
}

async fn catalog(
    context: RequestContext,
    cache: MemoryCache,
) -> Result<Response<Body>, HandlerError> {
    let policy = context
        .cache_policy("catalog-public")
        .expect("public cache policy is sealed");
    let key = public_key(policy).map_err(internal)?;
    let catalog = match context
        .data()
        .cache_lookup::<_, CatalogView>(&cache, &key)
        .await
        .map_err(internal)?
        .value
    {
        Some(value) => value,
        None => {
            let loader_policy = context
                .loader_policy("catalog-loader")
                .expect("catalog loader is sealed")
                .clone();
            let loader = |loader: pliego_runtime::LoaderContext, _query: CatalogQuery| async move {
                loader
                    .resource::<AccountStore>("accounts")?
                    .use_with(AccountStore::catalog)
            };
            let loaded = context
                .data()
                .load(&loader_policy, &loader, CatalogQuery { revision: 1 })
                .await
                .map_err(internal)?;
            let value = (*loaded).clone();
            context
                .data()
                .cache_insert(
                    &cache,
                    &key,
                    &value,
                    [CacheTag::new("account-public").map_err(internal)?],
                )
                .await
                .map_err(internal)?;
            value
        }
    };
    let items = catalog
        .names
        .iter()
        .map(|name| format!("<li>{}</li>", escape(name)))
        .collect::<String>();
    let mut response = html_response(
        StatusCode::OK,
        &page(
            "Public catalog",
            &format!("<h1>Public catalog</h1><ul>{items}</ul>"),
        ),
    )?;
    response.headers_mut().insert(
        http::header::CACHE_CONTROL,
        http::HeaderValue::from_static("public, max-age=60"),
    );
    Ok(response)
}

async fn account_profile(context: RequestContext) -> Result<Response<Body>, HandlerError> {
    let user_id = context
        .data()
        .values()
        .route_parameter("user_id")
        .ok_or_else(|| HandlerError::internal("sealed account route lost its parameter"))?
        .to_owned();
    let policy = context
        .loader_policy("public-account-loader")
        .expect("public account loader is sealed")
        .clone();
    let loader = |loader: pliego_runtime::LoaderContext, query: AccountQuery| async move {
        loader
            .resource::<AccountStore>("accounts")?
            .use_with(|store| store.find_account(&query.user_id))?
            .ok_or_else(|| DataError::LoaderFailure("account-not-found".to_owned()))
    };
    match context
        .data()
        .load(&policy, &loader, AccountQuery { user_id })
        .await
    {
        Ok(account) => html_response(
            StatusCode::OK,
            &page(
                "Account profile",
                &format!("<h1>{}</h1>", escape(&account.display_name)),
            ),
        ),
        Err(DataError::LoaderFailure(code)) if code == "account-not-found" => html_response(
            StatusCode::NOT_FOUND,
            &page(
                "Account not found",
                "<h1>Account not found</h1><p>The requested account does not exist.</p>",
            ),
        ),
        Err(error) => Err(internal(error)),
    }
}

async fn authenticated_session(
    sessions: &Sessions,
    token: &SessionToken,
) -> Result<pliego_runtime::LoadedSession<SessionClaims>, HandlerError> {
    let session = sessions
        .load::<SessionClaims>(token)
        .await
        .map_err(internal)?
        .ok_or_else(unauthorized)?;
    if !session.claims.authenticated || session.claims.role != "member" {
        return Err(unauthorized());
    }
    Ok(session)
}

fn required_token(
    request: &pliego_runtime::Request<Body>,
    sessions: &Sessions,
) -> Result<SessionToken, HandlerError> {
    read_session_token(request.headers(), sessions.policy().cookie_policy())
        .map_err(internal)?
        .ok_or_else(unauthorized)
}

fn public_key(policy: &CachePolicy) -> Result<CacheKey, pliego_runtime::CacheError> {
    policy.key(CacheKeyInput::new(
        "catalog-loader",
        1,
        digest("catalog-v1"),
    )?)
}

fn private_key(
    policy: &CachePolicy,
    user_id: &str,
    token: &SessionToken,
) -> Result<CacheKey, pliego_runtime::CacheError> {
    policy.key(
        CacheKeyInput::new("account-loader", 1, digest(user_id))?
            .partition(CachePartition::from_identity(token.as_cookie_value())?),
    )
}

fn digest(value: &str) -> String {
    Sha256::digest(value.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn html_response(status: StatusCode, body: &str) -> Result<Response<Body>, HandlerError> {
    Response::builder()
        .status(status)
        .header(http::header::CONTENT_TYPE, "text/html; charset=utf-8")
        .header(http::header::CACHE_CONTROL, "no-store")
        .body(Body::from(body.to_owned()))
        .map_err(|_| HandlerError::internal("HTML response could not be built"))
}

fn redirect(location: &str) -> Result<Response<Body>, HandlerError> {
    Response::builder()
        .status(StatusCode::SEE_OTHER)
        .header(http::header::LOCATION, location)
        .header(http::header::CACHE_CONTROL, "no-store")
        .body(Body::empty())
        .map_err(|_| HandlerError::internal("redirect response could not be built"))
}

fn typed_form_error(
    title: &str,
    return_to: &str,
    errors: &FieldErrors,
) -> Result<Response<Body>, HandlerError> {
    let list = errors
        .iter()
        .map(|(field, message)| {
            format!(
                "<li><strong>{}</strong>: {}</li>",
                escape(field),
                escape(message)
            )
        })
        .collect::<String>();
    html_response(
        StatusCode::UNPROCESSABLE_ENTITY,
        &page(
            title,
            &format!(
                "<h1>{}</h1><div role=\"alert\" tabindex=\"-1\"><ul>{list}</ul></div><p><a href=\"{}\">Return</a></p>",
                escape(title),
                escape(return_to)
            ),
        ),
    )
}

fn page(title: &str, content: &str) -> String {
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><link rel=\"icon\" href=\"/favicon.svg\" type=\"image/svg+xml\"><title>{}</title><style>body{{max-width:48rem;margin:4rem auto;padding:0 1rem;font:16px/1.5 system-ui}}form{{display:grid;gap:1rem;max-width:24rem}}label{{display:grid;gap:.35rem}}input,button{{font:inherit;padding:.7rem}}button{{cursor:pointer}}</style></head><body>{content}</body></html>",
        escape(title)
    )
}

fn favicon() -> Result<Response<Body>, HandlerError> {
    Response::builder()
        .status(StatusCode::OK)
        .header(http::header::CONTENT_TYPE, "image/svg+xml")
        .header(
            http::header::CACHE_CONTROL,
            "public, max-age=31536000, immutable",
        )
        .body(Body::from(include_str!(
            "../../../brand/pliegors-symbol.svg"
        )))
        .map_err(|_| HandlerError::internal("favicon response could not be built"))
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn valid_user_id(value: &str) -> bool {
    (3..=32).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn unauthorized() -> HandlerError {
    HandlerError::new(
        StatusCode::UNAUTHORIZED,
        RuntimeDiagnostic::new("PLG-AUTH-401", "authentication is required")
            .expect("static auth diagnostic is valid"),
    )
}

fn forbidden(code: &str, message: &str) -> HandlerError {
    HandlerError::new(
        StatusCode::FORBIDDEN,
        RuntimeDiagnostic::new(code, message).expect("static auth diagnostic is valid"),
    )
}

fn internal(error: impl std::fmt::Display) -> HandlerError {
    HandlerError::internal(error.to_string())
}

pub async fn serve_single(address: SocketAddr) -> AppResult<()> {
    let origin = std::env::var("PLIEGO_ORIGIN").unwrap_or_else(|_| format!("http://{address}"));
    let cluster = build_cluster_for_origin(&origin)?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    println!(
        "PliegoRS G2 reference listening on http://{}",
        listener.local_addr()?
    );
    cluster
        .first
        .serve(listener, async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
