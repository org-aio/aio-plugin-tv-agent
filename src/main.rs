mod agent_engine;
mod ai;
mod catalog;
mod config;
mod settings;
mod tvbox;

use std::{env, path::PathBuf, sync::Arc};

use serde::Serialize;
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

use crate::{ai::AiClient, config::AppConfig, settings::SettingsStore, tvbox::TvBoxClient};

#[derive(Clone)]
struct AppState {
    config: AppConfig,
    ai: AiClient,
    tvbox: TvBoxClient,
    settings: SettingsStore,
}

#[derive(Serialize)]
struct RuntimeContext {
    tenant_id: String,
    user_id: String,
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
    let ai = AiClient::new(
        config.ai_endpoint.clone(),
        config.ai_model.clone(),
        config.ai_key.clone(),
        config.broker.clone(),
    )
    .expect("初始化 AI 客户端失败");
    let tvbox = TvBoxClient::new(config.tvbox_config.clone(), config.broker.clone())
        .expect("初始化影视源客户端失败");
    let settings = SettingsStore::new(config.database_url.clone(), config.encryption_key)
        .await
        .expect("初始化电视 Agent 设置失败");
    let state = Arc::new(AppState {
        config,
        ai,
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
    let _ = cx;
    Ok(Json(catalog::catalog(query.q.as_deref())))
}

#[route(POST "/api/agent")]
async fn agent_reply(
    cx: &Cx,
    Json(request): Json<agent_engine::AgentRequest>,
) -> Result<Json<agent_engine::AgentReply>> {
    let state = topcoat::context::app_context::<Arc<AppState>>(cx);
    let user_id = header(headers(cx), "x-aio-user-id");
    let tenant_id = header(headers(cx), "x-aio-tenant-id");
    let stored_key = if user_id.is_empty() {
        None
    } else {
        state
            .settings
            .secret(&tenant_id, &user_id)
            .await
            .unwrap_or(None)
    };
    let ai = if stored_key.as_ref() == state.config.ai_key.as_ref() {
        state.ai.clone()
    } else {
        AiClient::new(
            state.config.ai_endpoint.clone(),
            state.config.ai_model.clone(),
            stored_key.or_else(|| state.config.ai_key.clone()),
            state.config.broker.clone(),
        )?
    };
    Ok(Json(
        agent_engine::respond(request, &ai, &state.tvbox).await,
    ))
}

#[route(GET "/api/settings")]
async fn settings_view(cx: &Cx) -> Result<Json<settings::SettingsView>> {
    let state = topcoat::context::app_context::<Arc<AppState>>(cx);
    let headers = headers(cx);
    let tenant = header(headers, "x-aio-tenant-id");
    let user = header(headers, "x-aio-user-id");
    Ok(Json(state.settings.read(&tenant, &user).await?))
}

#[route(POST "/api/settings")]
async fn settings_save(
    cx: &Cx,
    Json(draft): Json<settings::SettingsDraft>,
) -> Result<Json<settings::SettingsView>> {
    let state = topcoat::context::app_context::<Arc<AppState>>(cx);
    let headers = headers(cx);
    let tenant = header(headers, "x-aio-tenant-id");
    let user = header(headers, "x-aio-user-id");
    Ok(Json(state.settings.save(&tenant, &user, draft).await?))
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
