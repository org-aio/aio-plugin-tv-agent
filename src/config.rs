use std::{env, fs, path::PathBuf};

use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use reqwest::Url;
use serde::Deserialize;

pub(crate) const DEFAULT_AI_ENDPOINT: &str = "https://company-ai.addzero.site/v1";
pub(crate) const DEFAULT_AI_MODEL: &str = "auto";
pub(crate) const DEFAULT_TVBOX_CONFIGS: &[&str] = &[
    "https://raw.githubusercontent.com/TVboxorg/TVbox/main/dist/official.json",
    "https://raw.githubusercontent.com/wangguo0/tvbox-sub/main/merged.json",
    "https://raw.githubusercontent.com/ZHOUYU86/tvbox/main/b.json",
    "https://raw.githubusercontent.com/hebijunge/tvbox-config/main/tvbox.json",
    "https://raw.githubusercontent.com/haygcao/tvbox-master-aggregator/main/tvbox.json",
];

#[derive(Clone, Debug)]
pub(crate) struct AppConfig {
    pub ai_endpoint: String,
    pub ai_model: String,
    pub ai_key: Option<String>,
    pub model_endpoints: Vec<String>,
    pub tvbox_configs: Vec<String>,
    pub broker: Option<BrokerConfig>,
    pub encryption_key: Option<[u8; 32]>,
    pub database_url: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct BrokerConfig {
    pub socket: PathBuf,
    pub token: String,
    pub model_endpoints: Vec<String>,
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
        let configured_endpoint = env::var("AIO_TV_AGENT_AI_ENDPOINT")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_AI_ENDPOINT.to_owned());
        let configured_endpoint = normalize_endpoint(&configured_endpoint);
        let broker = grant.as_ref().map(|grant| BrokerConfig {
            socket: PathBuf::from(&grant.broker_socket),
            token: grant.ingress_token.clone(),
            model_endpoints: grant
                .endpoints
                .iter()
                .map(|endpoint| normalize_endpoint(endpoint))
                .collect(),
            http_endpoints: grant
                .http_endpoints
                .iter()
                .map(|endpoint| normalize_endpoint(endpoint))
                .collect(),
        });
        let ai_endpoint = broker
            .as_ref()
            .and_then(|broker| {
                broker
                    .model_endpoints
                    .iter()
                    .find(|endpoint| endpoint.as_str() == configured_endpoint)
                    .cloned()
            })
            .or_else(|| {
                broker
                    .as_ref()
                    .and_then(|broker| broker.model_endpoints.first().cloned())
            })
            .unwrap_or(configured_endpoint);

        let encryption_key = grant
            .as_ref()
            .and_then(|grant| grant.encryption_key.as_deref())
            .map(decode_key)
            .transpose()?;
        let database_url = grant.and_then(|grant| grant.database_url);

        let model_endpoints = broker
            .as_ref()
            .map(|broker| broker.model_endpoints.clone())
            .unwrap_or_else(|| vec![ai_endpoint.clone()]);
        let tvbox_configs = tvbox_configs()
            .into_iter()
            .filter(|config| {
                broker.as_ref().is_none_or(|broker| {
                    broker
                        .http_endpoints
                        .iter()
                        .any(|allowed| same_http_endpoint(allowed, config))
                })
            })
            .collect();

        Ok(Self {
            ai_endpoint,
            ai_model: env::var("AIO_TV_AGENT_AI_MODEL")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| DEFAULT_AI_MODEL.to_owned()),
            ai_key: env::var("AIO_TV_AGENT_AI_KEY")
                .ok()
                .filter(|value| !value.trim().is_empty()),
            model_endpoints,
            tvbox_configs,
            broker,
            encryption_key,
            database_url,
        })
    }

    pub(crate) fn validate_model_endpoint(&self, value: &str) -> Result<String> {
        let endpoint = normalize_endpoint(value);
        ensure!(
            !endpoint.is_empty() && endpoint.len() <= 2048,
            "模型 API 地址格式无效"
        );
        if let Some(broker) = &self.broker {
            ensure!(
                broker
                    .model_endpoints
                    .iter()
                    .any(|allowed| allowed == &endpoint),
                "模型地址未被 AIO 授权"
            );
            return Ok(endpoint);
        }

        let url = Url::parse(&endpoint).context("模型 API 地址格式无效")?;
        ensure!(
            url.scheme() == "https"
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none(),
            "模型 API 地址必须为不含凭据、查询或锚点的 HTTPS 地址"
        );
        Ok(endpoint)
    }

    pub(crate) fn validate_tvbox_config(&self, value: &str) -> Result<String> {
        let endpoint = normalize_endpoint(value);
        let url = Url::parse(&endpoint).context("TVBox 配置地址格式无效")?;
        ensure!(
            url.scheme() == "https"
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none(),
            "TVBox 配置地址必须为不含凭据、查询或锚点的 HTTPS 地址"
        );
        if let Some(broker) = &self.broker {
            ensure!(
                broker
                    .http_endpoints
                    .iter()
                    .any(|allowed| same_http_endpoint(allowed, &endpoint)),
                "TVBox 配置地址未被 AIO 授权"
            );
        }
        Ok(endpoint)
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

fn tvbox_configs() -> Vec<String> {
    env::var("AIO_TV_AGENT_TVBOX_CONFIGS")
        .ok()
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .filter(|values| !values.is_empty())
        .unwrap_or_else(|| {
            DEFAULT_TVBOX_CONFIGS
                .iter()
                .map(|value| (*value).to_owned())
                .collect()
        })
}

pub(crate) fn normalize_endpoint(value: &str) -> String {
    value.trim().trim_end_matches('/').to_owned()
}

pub(crate) fn same_http_endpoint(allowed: &str, requested: &str) -> bool {
    let Ok(allowed) = Url::parse(allowed) else {
        return false;
    };
    let Ok(requested) = Url::parse(requested) else {
        return false;
    };
    allowed.scheme() == requested.scheme()
        && allowed.host_str() == requested.host_str()
        && allowed.port_or_known_default() == requested.port_or_known_default()
        && allowed.path().trim_end_matches('/') == requested.path().trim_end_matches('/')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn direct_config() -> AppConfig {
        AppConfig {
            ai_endpoint: DEFAULT_AI_ENDPOINT.into(),
            ai_model: DEFAULT_AI_MODEL.into(),
            ai_key: None,
            model_endpoints: vec![DEFAULT_AI_ENDPOINT.into()],
            tvbox_configs: DEFAULT_TVBOX_CONFIGS
                .iter()
                .map(|value| (*value).to_owned())
                .collect(),
            broker: None,
            encryption_key: None,
            database_url: None,
        }
    }

    #[test]
    fn direct_mode_accepts_https_and_rejects_unsafe_endpoints() {
        let config = direct_config();
        assert_eq!(
            config
                .validate_model_endpoint(" https://example.com/v1/ ")
                .unwrap(),
            "https://example.com/v1"
        );
        assert!(
            config
                .validate_model_endpoint("http://example.com/v1")
                .is_err()
        );
        assert!(
            config
                .validate_model_endpoint("https://key@example.com/v1")
                .is_err()
        );
        assert!(
            config
                .validate_model_endpoint("https://example.com/v1?key=secret")
                .is_err()
        );
    }

    #[test]
    fn broker_mode_only_accepts_granted_endpoints() {
        let mut config = direct_config();
        config.broker = Some(BrokerConfig {
            socket: "/broker.sock".into(),
            token: "token".into(),
            model_endpoints: vec!["https://company-ai.addzero.site/v1".into()],
            http_endpoints: Vec::new(),
        });
        assert!(
            config
                .validate_model_endpoint("https://company-ai.addzero.site/v1")
                .is_ok()
        );
        assert!(
            config
                .validate_model_endpoint("https://unapproved.example/v1")
                .is_err()
        );
        assert!(
            config
                .validate_tvbox_config("https://example.com/tvbox.json")
                .is_err()
        );
        config.broker.as_mut().unwrap().http_endpoints =
            vec!["https://example.com/tvbox.json".into()];
        assert_eq!(
            config
                .validate_tvbox_config("https://example.com/tvbox.json")
                .unwrap(),
            "https://example.com/tvbox.json"
        );
    }
}
