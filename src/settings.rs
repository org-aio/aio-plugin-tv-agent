use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, OsRng, rand_core::RngCore},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};

#[derive(Clone)]
pub(crate) struct SettingsStore {
    pool: Option<PgPool>,
    encryption_key: Option<[u8; 32]>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub(crate) struct SettingsView {
    pub has_secret: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct SettingsDraft {
    #[serde(default)]
    pub secret: Option<String>,
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

    pub(crate) async fn read(&self, tenant: &str, user: &str) -> Result<SettingsView> {
        let Some(pool) = &self.pool else {
            return Ok(SettingsView::default());
        };
        let row = sqlx::query(
            "SELECT secret IS NOT NULL AS has_secret
             FROM tv_agent_settings WHERE tenant_id=$1 AND user_id=$2",
        )
        .bind(tenant)
        .bind(user)
        .fetch_optional(pool)
        .await?;
        Ok(SettingsView {
            has_secret: row.is_some_and(|row| row.get("has_secret")),
        })
    }

    pub(crate) async fn save(
        &self,
        tenant: &str,
        user: &str,
        draft: SettingsDraft,
    ) -> Result<SettingsView> {
        let Some(pool) = &self.pool else {
            anyhow::bail!("AIO 设置页仅在安装后的插件中可用");
        };
        let Some(secret) = draft.secret else {
            return self.read(tenant, user).await;
        };
        let secret = secret.trim();
        ensure!(
            secret.len() <= 8192 && !secret.contains(['\r', '\n']),
            "AI Key 格式无效"
        );
        if secret.is_empty() {
            sqlx::query("DELETE FROM tv_agent_settings WHERE tenant_id=$1 AND user_id=$2")
                .bind(tenant)
                .bind(user)
                .execute(pool)
                .await?;
            return Ok(SettingsView::default());
        }
        let encrypted = encrypt(
            self.encryption_key.context("AIO 未授权加密能力")?,
            secret,
            &owner(tenant, user),
        )?;
        sqlx::query(
            "INSERT INTO tv_agent_settings(tenant_id,user_id,secret)
             VALUES($1,$2,$3)
             ON CONFLICT (tenant_id,user_id)
             DO UPDATE SET secret=EXCLUDED.secret",
        )
        .bind(tenant)
        .bind(user)
        .bind(encrypted)
        .execute(pool)
        .await?;
        Ok(SettingsView { has_secret: true })
    }

    pub(crate) async fn secret(&self, tenant: &str, user: &str) -> Result<Option<String>> {
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
