mod agent_engine;
mod catalog;

use std::env;

use serde::Serialize;
use topcoat::{
    Result,
    context::Cx,
    router::{
        Router, RouterBuilderDiscoverExt,
        content::{Form, Json},
        request::headers,
        route,
    },
};

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
            topcoat::serve(listener, router()).await.unwrap();
            return;
        }
        #[cfg(not(unix))]
        {
            let _ = path;
        }
    }
    topcoat::start(router()).await.unwrap();
}

fn router() -> Router {
    Router::builder().discover().build()
}

#[route(GET "/health")]
async fn health() -> Result<&'static str> {
    Ok("ok")
}

#[route(GET "/aio/describe")]
async fn describe() -> Result<Json<serde_json::Value>> {
    Ok(Json(serde_json::json!({
        "label": "电视 Agent",
        "pages": [{
            "id": "tv-agent",
            "label": "电视 Agent",
            "entry": "index.html",
            "scene": ["workspace", "工作空间"],
            "menu_path": ["电视 Agent"],
            "permission": null,
            "surface": "workspace"
        }]
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
    Json(request): Json<agent_engine::AgentRequest>,
) -> Result<Json<agent_engine::AgentReply>> {
    Ok(Json(agent_engine::respond(request)))
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

    async fn request(method: Method, path: &str, body: Body) -> (StatusCode, String) {
        let request = Request::builder()
            .method(method)
            .uri(path)
            .header("content-type", "application/json")
            .header("x-aio-tenant-id", "tenant-test")
            .header("x-aio-user-id", "user-test")
            .body(body)
            .unwrap();
        let response = router().handle(request).await;
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (status, String::from_utf8(bytes.to_vec()).unwrap())
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
    async fn agent_selects_playable_episode() {
        let (status, body) = request(
            Method::POST,
            "/api/agent",
            Body::from("{\"message\":\"我想看治愈的动物短剧\"}"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("\"intent\":\"play\""));
        assert!(body.contains("\"id\":\"llama-drama\""));
        assert!(body.contains("\"video\":\"assets/videos/llama-drama.mp4\""));
    }
}
