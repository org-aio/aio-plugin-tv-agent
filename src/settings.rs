use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, OsRng, rand_core::RngCore},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};

use crate::config::normalize_endpoint;

#[derive(Clone)]
pub(crate) struct SettingsStore {
    pool: Option<PgPool>,
    encryption_key: Option<[u8; 32]>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub(crate) struct SettingsView {
    pub model_endpoint: String,
    pub model: String,
    pub has_secret: bool,
    pub tvbox_configs: Vec<String>,
    pub danmaku_api: String,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct SettingsDraft {
    pub model_endpoint: String,
    pub model: String,
    #[serde(default)]
    pub secret: Option<String>,
    #[serde(default)]
    pub tvbox_configs: Option<Vec<String>>,
    #[serde(default)]
    pub danmaku_api: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct RuntimeSettings {
    pub model_endpoint: String,
    pub model: String,
    pub secret: Option<String>,
    pub tvbox_configs: Vec<String>,
}

impl SettingsStore {
    pub(crate) async fn new(
        database_url: Option<String>,
        encryption_key: Option<[u8; 32]>,
    ) -> Result<Self> {
        let pool = match database_url {
            Some(url) => Some(PgPool::connect(&url).await.context("连接插件数据库失败")?),
            None => None,
        };
        if let Some(pool) = &pool {
            sqlx::query(
                "CREATE TABLE IF NOT EXISTS tv_agent_settings (
                    tenant_id TEXT NOT NULL,
                    user_id TEXT NOT NULL,
                    secret BYTEA,
                    model_endpoint TEXT,
                    model TEXT,
                    tvbox_configs TEXT[],
                    danmaku_api TEXT,
                    PRIMARY KEY (tenant_id, user_id)
                )",
            )
            .execute(pool)
            .await
            .context("初始化电视 Agent 设置失败")?;
        }
        Ok(Self {
            pool,
            encryption_key,
        })
    }

    pub(crate) async fn read(
        &self,
        tenant: &str,
        user: &str,
        default_endpoint: &str,
        default_model: &str,
        default_tvbox_configs: &[String],
    ) -> Result<SettingsView> {
        let Some(pool) = &self.pool else {
            return Ok(SettingsView {
                model_endpoint: default_endpoint.to_owned(),
                model: default_model.to_owned(),
                has_secret: false,
                tvbox_configs: default_tvbox_configs.to_vec(),
                danmaku_api: String::new(),
            });
        };
        let row = sqlx::query(
            "SELECT secret IS NOT NULL AS has_secret, model_endpoint, model, tvbox_configs, danmaku_api
             FROM tv_agent_settings WHERE tenant_id=$1 AND user_id=$2",
        )
        .bind(tenant)
        .bind(user)
        .fetch_optional(pool)
        .await?;
        let Some(row) = row else {
            return Ok(SettingsView {
                model_endpoint: default_endpoint.to_owned(),
                model: default_model.to_owned(),
                has_secret: false,
                tvbox_configs: default_tvbox_configs.to_vec(),
                danmaku_api: String::new(),
            });
        };
        Ok(SettingsView {
            model_endpoint: row
                .get::<Option<String>, _>("model_endpoint")
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| default_endpoint.to_owned()),
            model: row
                .get::<Option<String>, _>("model")
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| default_model.to_owned()),
            has_secret: row.get("has_secret"),
            tvbox_configs: row
                .get::<Option<Vec<String>>, _>("tvbox_configs")
                .filter(|values| !values.is_empty())
                .unwrap_or_else(|| default_tvbox_configs.to_vec()),
            danmaku_api: row
                .get::<Option<String>, _>("danmaku_api")
                .unwrap_or_default(),
        })
    }

    pub(crate) async fn save(
        &self,
        tenant: &str,
        user: &str,
        draft: SettingsDraft,
        default_endpoint: &str,
        default_model: &str,
        default_tvbox_configs: &[String],
    ) -> Result<SettingsView> {
        let Some(pool) = &self.pool else {
            anyhow::bail!("AIO 设置页仅在安装后的插件中可用");
        };
        let model_endpoint = normalize_endpoint(&draft.model_endpoint);
        let model = draft.model.trim();
        ensure!(
            !model_endpoint.is_empty() && model_endpoint.len() <= 2048,
            "模型 API 地址格式无效"
        );
        ensure!(!model.is_empty() && model.len() <= 160, "模型名格式无效");
        let tvbox_configs = draft
            .tvbox_configs
            .unwrap_or_else(|| default_tvbox_configs.to_vec());
        ensure!(
            !tvbox_configs.is_empty() && tvbox_configs.len() <= 16,
            "TVBox 配置地址数量必须为 1 到 16 个"
        );
        for config in &tvbox_configs {
            let url = reqwest::Url::parse(config).context("TVBox 配置地址格式无效")?;
            ensure!(
                url.scheme() == "https"
                    && url.host_str().is_some()
                    && url.username().is_empty()
                    && url.password().is_none(),
                "TVBox 配置地址必须是 HTTPS 地址"
            );
        }
        let danmaku_api = validate_optional_https_url(
            draft.danmaku_api.as_deref().unwrap_or_default(),
            "弹幕接口",
        )?;

        let existing = sqlx::query(
            "SELECT secret, model_endpoint FROM tv_agent_settings
             WHERE tenant_id=$1 AND user_id=$2",
        )
        .bind(tenant)
        .bind(user)
        .fetch_optional(pool)
        .await?;
        let existing_endpoint = existing
            .as_ref()
            .and_then(|row| row.get::<Option<String>, _>("model_endpoint"));
        let existing_secret = existing.and_then(|row| row.get::<Option<Vec<u8>>, _>("secret"));
        let encrypted = match draft.secret {
            Some(secret) => {
                let secret = secret.trim();
                ensure!(
                    secret.len() <= 8192 && !secret.contains(['\r', '\n']),
                    "AI Key 格式无效"
                );
                if secret.is_empty() {
                    None
                } else {
                    Some(encrypt(
                        self.encryption_key.context("AIO 未授权加密能力")?,
                        secret,
                        &owner(tenant, user),
                    )?)
                }
            }
            None if existing_endpoint
                .as_deref()
                .is_none_or(|endpoint| endpoint.trim_end_matches('/') == model_endpoint) =>
            {
                existing_secret
            }
            None => None,
        };
        sqlx::query(
            "INSERT INTO tv_agent_settings(tenant_id,user_id,secret,model_endpoint,model,tvbox_configs,danmaku_api)
             VALUES($1,$2,$3,$4,$5,$6,$7)
             ON CONFLICT (tenant_id,user_id)
             DO UPDATE SET secret=EXCLUDED.secret,
                           model_endpoint=EXCLUDED.model_endpoint,
                           model=EXCLUDED.model,
                           tvbox_configs=EXCLUDED.tvbox_configs,
                           danmaku_api=EXCLUDED.danmaku_api",
        )
        .bind(tenant)
        .bind(user)
        .bind(encrypted)
        .bind(&model_endpoint)
        .bind(model)
        .bind(&tvbox_configs)
        .bind(&danmaku_api)
        .execute(pool)
        .await?;
        self.read(
            tenant,
            user,
            default_endpoint,
            default_model,
            default_tvbox_configs,
        )
        .await
    }

    pub(crate) async fn runtime(
        &self,
        tenant: &str,
        user: &str,
        default_endpoint: &str,
        default_model: &str,
        default_tvbox_configs: &[String],
    ) -> Result<RuntimeSettings> {
        let view = self
            .read(
                tenant,
                user,
                default_endpoint,
                default_model,
                default_tvbox_configs,
            )
            .await?;
        Ok(RuntimeSettings {
            model_endpoint: view.model_endpoint,
            model: view.model,
            secret: self.secret(tenant, user).await?,
            tvbox_configs: view.tvbox_configs,
        })
    }

    async fn secret(&self, tenant: &str, user: &str) -> Result<Option<String>> {
        let Some(pool) = &self.pool else {
            return Ok(None);
        };
        let encrypted: Option<Vec<u8>> = sqlx::query_scalar(
            "SELECT secret FROM tv_agent_settings WHERE tenant_id=$1 AND user_id=$2",
        )
        .bind(tenant)
        .bind(user)
        .fetch_optional(pool)
        .await?
        .flatten();
        encrypted
            .map(|value| {
                decrypt(
                    self.encryption_key.context("AIO 未授权加密能力")?,
                    &value,
                    &owner(tenant, user),
                )
            })
            .transpose()
    }
}

fn owner(tenant: &str, user: &str) -> Vec<u8> {
    serde_json::to_vec(&(tenant, user, "tv-agent-ai-key")).expect("字符串序列化")
}

fn validate_optional_https_url(value: &str, label: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(String::new());
    }
    let url = reqwest::Url::parse(value).with_context(|| format!("{label}格式无效"))?;
    ensure!(
        url.scheme() == "https"
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.fragment().is_none(),
        "{label}必须是无凭据、无锚点的 HTTPS 地址"
    );
    Ok(value.to_owned())
}

fn encrypt(key: [u8; 32], secret: &str, owner: &[u8]) -> Result<Vec<u8>> {
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|_| anyhow::anyhow!("密钥无效"))?;
    let mut nonce = [0u8; 12];
    OsRng.fill_bytes(&mut nonce);
    let encrypted = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            aes_gcm::aead::Payload {
                msg: secret.as_bytes(),
                aad: owner,
            },
        )
        .map_err(|_| anyhow::anyhow!("AI Key 加密失败"))?;
    Ok([nonce.to_vec(), encrypted].concat())
}

fn decrypt(key: [u8; 32], value: &[u8], owner: &[u8]) -> Result<String> {
    ensure!(value.len() >= 28, "AI Key 密文无效");
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|_| anyhow::anyhow!("密钥无效"))?;
    let bytes = cipher
        .decrypt(
            Nonce::from_slice(&value[..12]),
            aes_gcm::aead::Payload {
                msg: &value[12..],
                aad: owner,
            },
        )
        .map_err(|_| anyhow::anyhow!("AI Key 解密失败"))?;
    String::from_utf8(bytes).context("AI Key 编码无效")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_is_bound_to_the_current_user() {
        let encrypted = encrypt([7; 32], "secret-value", &owner("tenant", "user")).unwrap();
        assert_eq!(
            decrypt([7; 32], &encrypted, &owner("tenant", "user")).unwrap(),
            "secret-value"
        );
        assert!(decrypt([7; 32], &encrypted, &owner("tenant", "other")).is_err());
    }
}
