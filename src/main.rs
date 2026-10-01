mod agent_engine;
mod ai;
mod catalog;
mod config;
mod danmaku_api;
mod settings;
mod tvbox;

use std::{env, path::PathBuf, sync::Arc};

use serde::{Deserialize, Serialize};
use topcoat::{
    Result,
    context::Cx,
    router::{
        Methods, Path, Router, RouterBuilderDiscoverExt,
        content::{Form, Json},
        request::headers,
        route,
        tower::TowerRoute,
    },
};
use tower_http::services::{ServeDir, ServeFile};

use crate::{
    ai::{AiClient, ModelTestRequest},
    config::AppConfig,
    settings::{SettingsStore, SettingsView},
    tvbox::TvBoxClient,
};

#[derive(Clone)]
struct AppState {
    config: AppConfig,
    tvbox: TvBoxClient,
    settings: SettingsStore,
}

#[derive(Serialize)]
struct RuntimeContext {
    tenant_id: String,
    user_id: String,
}

#[derive(Serialize)]
struct SettingsResponse {
    #[serde(flatten)]
    settings: SettingsView,
    allowed_endpoints: Vec<String>,
}

#[derive(serde::Deserialize)]
struct ModelListRequest {
    model_endpoint: String,
    #[serde(default)]
    secret: Option<String>,
}

#[derive(serde::Deserialize)]
struct DanmakuRequest {
    title: String,
    #[serde(default)]
    episode: String,
}

#[derive(Deserialize)]
struct BrowseQuery {
    #[serde(default)]
    category: String,
    #[serde(default = "default_page")]
    page: u32,
    #[serde(default = "default_page_size")]
    page_size: usize,
}

#[derive(Serialize)]
struct DanmakuPage {
    items: Vec<danmaku_api::DanmakuItem>,
}

fn default_page() -> u32 {
    1
}

fn default_page_size() -> usize {
    24
}

#[tokio::main]
async fn main() {
    if let Ok(port) = env::var("AIO_PLUGIN_PORT") {
        // Topcoat 使用 PORT；AIO 隔离进程只注入 AIO_PLUGIN_PORT。
        unsafe { env::set_var("PORT", port) };
    }
    if env::var_os("HOST").is_none() {
        // 本地开发统一监听 IPv4，确保 README 中的 127.0.0.1 地址可直接访问。
        unsafe { env::set_var("HOST", "127.0.0.1") };
    }
    if let Ok(path) = env::var("AIO_PLUGIN_SOCKET") {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let path = std::path::PathBuf::from(path);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => panic!("清理 Topcoat socket 失败: {error}"),
            }
            let listener = tokio::net::UnixListener::bind(&path).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).unwrap();
            topcoat::serve(listener, router().await).await.unwrap();
            return;
        }
        #[cfg(not(unix))]
        {
            let _ = path;
        }
    }
    topcoat::start(router().await).await.unwrap();
}

async fn router() -> Router {
    let config = AppConfig::load().expect("加载电视 Agent 配置失败");
    let tvbox = TvBoxClient::new(config.broker.clone()).expect("初始化影视源客户端失败");
    let settings = SettingsStore::new(config.database_url.clone(), config.encryption_key)
        .await
        .expect("初始化电视 Agent 设置失败");
    let state = Arc::new(AppState {
        config,
        tvbox,
        settings,
    });
    let mut builder = Router::builder().app_context(state).discover();
    if let Some(frontend) = frontend_root() {
        let index = frontend.join("index.html");
        let static_files = ServeDir::new(frontend).fallback(ServeFile::new(index));
        builder = builder
            .route(TowerRoute::new(
                Methods::Any,
                Path::new("/"),
                static_files.clone(),
            ))
            .route(TowerRoute::new(
                Methods::Any,
                Path::new("/{*path}"),
                static_files,
            ));
    }
    builder.build()
}

#[route(POST "/api/danmaku")]
async fn danmaku_view(cx: &Cx, Json(request): Json<DanmakuRequest>) -> Result<Json<DanmakuPage>> {
    let state = topcoat::context::app_context::<Arc<AppState>>(cx);
    let headers = headers(cx);
    let tenant = header(headers, "x-aio-tenant-id");
    let user = header(headers, "x-aio-user-id");
    let settings = state
        .settings
        .read(
            &tenant,
            &user,
            &state.config.ai_endpoint,
            &state.config.ai_model,
            &state.config.tvbox_configs,
        )
        .await?;
    let title = request.title.trim();
    if title.is_empty() {
        return Err(anyhow::anyhow!("视频标题不能为空").into());
    }
    if settings.danmaku_api.is_empty() {
        return Ok(Json(DanmakuPage { items: Vec::new() }));
    }
    let mut items = danmaku_api::load(
        &settings.danmaku_api,
        title,
        request.episode.trim(),
        state.config.broker.clone(),
    )
    .await?;
    items.sort_by(|left, right| left.time.total_cmp(&right.time));
    Ok(Json(DanmakuPage { items }))
}

fn frontend_root() -> Option<PathBuf> {
    let configured = env::var_os("AIO_PLUGIN_FRONTEND").map(PathBuf::from);
    let candidates = configured.into_iter().chain([
        PathBuf::from("frontend"),
        PathBuf::from("dist/frontend"),
        PathBuf::from("/plugin/frontend"),
    ]);
    candidates
        .into_iter()
        .find(|path| path.join("index.html").is_file())
}

#[route(GET "/health")]
async fn health() -> Result<&'static str> {
    Ok("ok")
}

#[route(GET "/aio/describe")]
async fn describe() -> Result<Json<serde_json::Value>> {
    Ok(Json(serde_json::json!({
        "label": "电视 Agent",
        "pages": [
            {
                "id": "tv-agent",
                "label": "电视 Agent",
                "entry": "index.html",
                "scene": ["workspace", "工作空间"],
                "menu_path": ["电视 Agent"],
                "permission": null,
                "surface": "workspace"
            },
            {
                "id": "settings",
                "label": "电视 Agent 设置",
                "entry": "settings.html",
                "scene": null,
                "menu_path": [],
                "permission": null,
                "surface": "fullscreen"
            }
        ]
    })))
}

#[route(GET "/api/catalog")]
async fn catalog_view(
    cx: &Cx,
    query: Form<catalog::CatalogQuery>,
) -> Result<Json<catalog::CatalogView>> {
    let state = topcoat::context::app_context::<Arc<AppState>>(cx);
    let query = query
        .q
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(query) = query {
        let headers = headers(cx);
        let tenant = header(headers, "x-aio-tenant-id");
        let user = header(headers, "x-aio-user-id");
        let settings = state
            .settings
            .read(
                &tenant,
                &user,
                &state.config.ai_endpoint,
                &state.config.ai_model,
                &state.config.tvbox_configs,
            )
            .await?;
        if let Ok(Some(drama)) = state.tvbox.search(query, &settings.tvbox_configs).await {
            return Ok(Json(catalog::view(vec![drama])));
        }
    }
    Ok(Json(catalog::catalog(query)))
}

#[route(GET "/api/browse")]
async fn browse_view(cx: &Cx, query: Form<BrowseQuery>) -> Result<Json<tvbox::BrowsePage>> {
    let state = topcoat::context::app_context::<Arc<AppState>>(cx);
    let headers = headers(cx);
    let tenant = header(headers, "x-aio-tenant-id");
    let user = header(headers, "x-aio-user-id");
    let settings = state
        .settings
        .read(
            &tenant,
            &user,
            &state.config.ai_endpoint,
            &state.config.ai_model,
            &state.config.tvbox_configs,
        )
        .await?;
    Ok(Json(
        state
            .tvbox
            .browse(
                &settings.tvbox_configs,
                &query.category,
                query.page,
                query.page_size,
            )
            .await?,
    ))
}

#[route(POST "/api/agent")]
async fn agent_reply(
    cx: &Cx,
    Json(request): Json<agent_engine::AgentRequest>,
) -> Result<Json<agent_engine::AgentReply>> {
    let state = topcoat::context::app_context::<Arc<AppState>>(cx);
    let user_id = header(headers(cx), "x-aio-user-id");
    let tenant_id = header(headers(cx), "x-aio-tenant-id");
    let settings = state
        .settings
        .runtime(
            &tenant_id,
            &user_id,
            &state.config.ai_endpoint,
            &state.config.ai_model,
            &state.config.tvbox_configs,
        )
        .await?;
    state
        .config
        .validate_model_endpoint(&settings.model_endpoint)?;
    let settings_endpoint_is_default = settings.model_endpoint == state.config.ai_endpoint;
    let default_secret = state.config.ai_key.clone();
    let ai = AiClient::new(
        settings.model_endpoint,
        settings.model,
        settings.secret.or_else(|| {
            // 默认 Key 只属于默认地址，不能随用户自定义地址外发。
            settings_endpoint_is_default
                .then_some(default_secret)
                .flatten()
        }),
        state.config.broker.clone(),
    )?;
    Ok(Json(
        agent_engine::respond(request, &ai, &state.tvbox, &settings.tvbox_configs).await,
    ))
}

#[route(GET "/api/settings")]
async fn settings_view(cx: &Cx) -> Result<Json<SettingsResponse>> {
    let state = topcoat::context::app_context::<Arc<AppState>>(cx);
    let headers = headers(cx);
    let tenant = header(headers, "x-aio-tenant-id");
    let user = header(headers, "x-aio-user-id");
    let settings = state
        .settings
        .read(
            &tenant,
            &user,
            &state.config.ai_endpoint,
            &state.config.ai_model,
            &state.config.tvbox_configs,
        )
        .await?;
    Ok(Json(SettingsResponse {
        settings,
        allowed_endpoints: state.config.model_endpoints.clone(),
    }))
}

#[route(POST "/api/settings")]
async fn settings_save(
    cx: &Cx,
    Json(draft): Json<settings::SettingsDraft>,
) -> Result<Json<SettingsResponse>> {
    let state = topcoat::context::app_context::<Arc<AppState>>(cx);
    let headers = headers(cx);
    let tenant = header(headers, "x-aio-tenant-id");
    let user = header(headers, "x-aio-user-id");
    let model_endpoint = state
        .config
        .validate_model_endpoint(&draft.model_endpoint)?;
    let mut draft = draft;
    draft.model_endpoint = model_endpoint;
    let tvbox_configs = draft
        .tvbox_configs
        .take()
        .unwrap_or_else(|| state.config.tvbox_configs.clone());
    draft.tvbox_configs = Some(
        tvbox_configs
            .iter()
            .map(|config| state.config.validate_tvbox_config(config))
            .collect::<anyhow::Result<Vec<_>>>()?,
    );
    let settings = state
        .settings
        .save(
            &tenant,
            &user,
            draft,
            &state.config.ai_endpoint,
            &state.config.ai_model,
            &state.config.tvbox_configs,
        )
        .await?;
    Ok(Json(SettingsResponse {
        settings,
        allowed_endpoints: state.config.model_endpoints.clone(),
    }))
}

#[route(POST "/api/models")]
async fn models(cx: &Cx, Json(draft): Json<ModelListRequest>) -> Result<Json<Vec<String>>> {
    let state = topcoat::context::app_context::<Arc<AppState>>(cx);
    let headers = headers(cx);
    let tenant = header(headers, "x-aio-tenant-id");
    let user = header(headers, "x-aio-user-id");
    let endpoint = state
        .config
        .validate_model_endpoint(&draft.model_endpoint)?;
    let saved = state
        .settings
        .runtime(
            &tenant,
            &user,
            &state.config.ai_endpoint,
            &state.config.ai_model,
            &state.config.tvbox_configs,
        )
        .await?;
    let secret = draft
        .secret
        .map(validate_secret)
        .transpose()?
        .flatten()
        .or_else(|| {
            (endpoint == saved.model_endpoint)
                .then_some(saved.secret)
                .flatten()
        })
        .or_else(|| {
            (endpoint == state.config.ai_endpoint)
                .then(|| state.config.ai_key.clone())
                .flatten()
        });
    let ai = AiClient::new(endpoint, saved.model, secret, state.config.broker.clone())?;
    Ok(Json(ai.list_models(None).await?))
}

#[route(POST "/api/models/test")]
async fn model_test(
    cx: &Cx,
    Json(draft): Json<ModelTestRequest>,
) -> Result<Json<serde_json::Value>> {
    let state = topcoat::context::app_context::<Arc<AppState>>(cx);
    let headers = headers(cx);
    let tenant = header(headers, "x-aio-tenant-id");
    let user = header(headers, "x-aio-user-id");
    let endpoint = state
        .config
        .validate_model_endpoint(&draft.model_endpoint)?;
    let model = draft.model.trim();
    if model.is_empty() || model.len() > 160 {
        return Err(anyhow::anyhow!("模型名格式无效").into());
    }
    let saved = state
        .settings
        .runtime(
            &tenant,
            &user,
            &state.config.ai_endpoint,
            &state.config.ai_model,
            &state.config.tvbox_configs,
        )
        .await?;
    let secret = draft
        .secret
        .map(validate_secret)
        .transpose()?
        .flatten()
        .or_else(|| {
            (endpoint == saved.model_endpoint)
                .then_some(saved.secret)
                .flatten()
        })
        .or_else(|| {
            (endpoint == state.config.ai_endpoint)
                .then(|| state.config.ai_key.clone())
                .flatten()
        });
    let ai = AiClient::new(
        endpoint.clone(),
        model.to_owned(),
        secret,
        state.config.broker.clone(),
    )?;
    ai.test_connection(None).await?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "model_endpoint": endpoint,
        "model": model,
    })))
}

#[route(GET "/api/context")]
async fn context(cx: &Cx) -> Result<Json<RuntimeContext>> {
    let headers = headers(cx);
    Ok(Json(RuntimeContext {
        tenant_id: header(headers, "x-aio-tenant-id"),
        user_id: header(headers, "x-aio-user-id"),
    }))
}

fn header(headers: &topcoat::router::HeaderMap, name: &str) -> String {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned()
}

fn validate_secret(secret: String) -> Result<Option<String>> {
    let secret = secret.trim();
    if secret.is_empty() {
        return Ok(None);
    }
    if secret.len() > 8192 || secret.contains(['\r', '\n']) {
        return Err(anyhow::anyhow!("AI Key 格式无效").into());
    }
    Ok(Some(secret.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use topcoat::router::{Body, Method, StatusCode, request::Request, to_bytes};

    async fn request_bytes(method: Method, path: &str, body: Body) -> (StatusCode, Vec<u8>) {
        let request = Request::builder()
            .method(method)
            .uri(path)
            .header("content-type", "application/json")
            .header("x-aio-tenant-id", "tenant-test")
            .header("x-aio-user-id", "user-test")
            .body(body)
            .unwrap();
        let response = router().await.handle(request).await;
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (status, bytes.to_vec())
    }

    async fn request(method: Method, path: &str, body: Body) -> (StatusCode, String) {
        let (status, bytes) = request_bytes(method, path, body).await;
        (status, String::from_utf8(bytes).unwrap())
    }

    #[tokio::test]
    async fn exposes_aio_runtime_contract() {
        let (status, body) = request(Method::GET, "/health", Body::empty()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, "ok");

        let (status, body) = request(Method::GET, "/aio/describe", Body::empty()).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("\"id\":\"tv-agent\""));
        assert!(body.contains("\"entry\":\"index.html\""));
    }

    #[tokio::test]
    async fn returns_catalog_and_host_context() {
        let (status, body) = request(Method::GET, "/api/catalog", Body::empty()).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("\"forest-run\""));
        assert!(body.contains("assets/videos/forest-run.mp4"));

        let (status, body) = request(Method::GET, "/api/context", Body::empty()).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("tenant-test"));
        assert!(body.contains("user-test"));
    }

    #[tokio::test]
    async fn serves_frontend_and_api_from_one_origin() {
        let (status, body) = request(Method::GET, "/", Body::empty()).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("电视 Agent"));

        let (status, body) =
            request_bytes(Method::GET, "/assets/posters/forest-run.jpg", Body::empty()).await;
        assert_eq!(status, StatusCode::OK);
        assert!(!body.is_empty());

        let (status, body) = request(Method::GET, "/api/catalog", Body::empty()).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("\"forest-run\""));
    }

    #[tokio::test]
    async fn agent_selects_playable_episode() {
        let (status, body) = request(
            Method::POST,
            "/api/agent",
            Body::from("{\"message\":\"播放森林狂奔\"}"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("\"intent\":\"play\""));
        assert!(body.contains("\"id\":\"forest-run\""));
        assert!(body.contains("\"video\":\"assets/videos/forest-run.mp4\""));
    }
}
