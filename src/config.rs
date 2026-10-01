use std::{env, fs, path::PathBuf};

use anyhow::{Context, Result};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Deserialize;

pub(crate) const DEFAULT_AI_ENDPOINT: &str = "https://company-ai.addzero.site/v1";
pub(crate) const DEFAULT_AI_MODEL: &str = "cn:fast-model";
pub(crate) const DEFAULT_TVBOX_CONFIG: &str = "https://szyyds.cn/tv/x.json";

#[derive(Clone, Debug)]
pub(crate) struct AppConfig {
    pub ai_endpoint: String,
    pub ai_model: String,
    pub ai_key: Option<String>,
    pub tvbox_config: String,
    pub broker: Option<BrokerConfig>,
    pub encryption_key: Option<[u8; 32]>,
    pub database_url: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct BrokerConfig {
    pub socket: PathBuf,
    pub token: String,
    pub model_endpoint: String,
    pub http_endpoints: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct HostGrant {
    #[serde(default)]
    database_url: Option<String>,
    encryption_key: Option<String>,
    ingress_token: String,
    broker_socket: String,
    #[serde(default)]
    endpoints: Vec<String>,
    #[serde(default)]
    http_endpoints: Vec<String>,
}

impl AppConfig {
    pub(crate) fn load() -> Result<Self> {
        let grant = load_host_grant()?;
        let ai_endpoint = env::var("AIO_TV_AGENT_AI_ENDPOINT")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_AI_ENDPOINT.to_owned());
        let ai_endpoint = normalize_endpoint(&ai_endpoint);
        let model_endpoint = grant
            .as_ref()
            .and_then(|grant| {
                grant
                    .endpoints
                    .iter()
                    .any(|allowed| normalize_endpoint(allowed) == ai_endpoint)
                    .then_some(ai_endpoint.clone())
            })
            .unwrap_or(ai_endpoint);

        let broker = grant.as_ref().and_then(|grant| {
            grant
                .endpoints
                .iter()
                .any(|allowed| normalize_endpoint(allowed) == model_endpoint)
                .then(|| BrokerConfig {
                    socket: PathBuf::from(&grant.broker_socket),
                    token: grant.ingress_token.clone(),
                    model_endpoint: model_endpoint.clone(),
                    http_endpoints: grant
                        .http_endpoints
                        .iter()
                        .map(|endpoint| normalize_endpoint(endpoint))
                        .collect(),
                })
        });

        let encryption_key = grant
            .as_ref()
            .and_then(|grant| grant.encryption_key.as_deref())
            .map(decode_key)
            .transpose()?;
        let database_url = grant.and_then(|grant| grant.database_url);

        Ok(Self {
            ai_endpoint: model_endpoint,
            ai_model: env::var("AIO_TV_AGENT_AI_MODEL")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| DEFAULT_AI_MODEL.to_owned()),
            ai_key: env::var("AIO_TV_AGENT_AI_KEY")
                .ok()
                .filter(|value| !value.trim().is_empty()),
            tvbox_config: env::var("AIO_TV_AGENT_TVBOX_CONFIG")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| DEFAULT_TVBOX_CONFIG.to_owned()),
            broker,
            encryption_key,
            database_url,
        })
    }
}

fn load_host_grant() -> Result<Option<HostGrant>> {
    let Some(path) = env::var_os("AIO_PLUGIN_CONFIG") else {
        return Ok(None);
    };
    let bytes = fs::read(&path)
        .with_context(|| format!("读取 AIO 插件配置失败: {}", PathBuf::from(&path).display()))?;
    serde_json::from_slice(&bytes)
        .context("解析 AIO 插件配置失败")
        .map(Some)
}

fn decode_key(value: &str) -> Result<[u8; 32]> {
    STANDARD
        .decode(value)
        .context("AIO 加密密钥不是有效 Base64")?
        .try_into()
        .map_err(|_| anyhow::anyhow!("AIO 加密密钥必须为 32 字节"))
}

fn normalize_endpoint(value: &str) -> String {
    value.trim().trim_end_matches('/').to_owned()
}
