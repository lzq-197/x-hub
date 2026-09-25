//! OpenAI 兼容 `/embeddings` 客户端（Ollama / OpenAI / 硅基流动 / 智谱等）。

use serde_json::Value;

const KEYRING_SERVICE: &str = "x-hub-embed";
const KEYRING_USER: &str = "default";
const BATCH: usize = 16;

pub struct EmbedConfig {
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
}

fn key_file_path() -> std::path::PathBuf {
    crate::config::config_dir().join("embed_keys.json")
}

pub fn save_embed_api_key(key: &str) -> Result<(), String> {
    crate::credentials::migrate_to_keyring(&key_file_path(), KEYRING_SERVICE, Some(KEYRING_USER))?;
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).map_err(|e| e.to_string())?;
    entry
        .set_password(key)
        .map_err(|e| format!("钥匙串写入失败，未保存明文: {e}"))
}

pub fn get_embed_api_key() -> Option<String> {
    if let Err(e) =
        crate::credentials::migrate_to_keyring(&key_file_path(), KEYRING_SERVICE, Some(KEYRING_USER))
    {
        log::warn!("旧嵌入凭据迁移未完成: {e}");
    }
    match keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER) {
        Ok(e) => e.get_password().ok(),
        Err(_) => None,
    }
}

/// 显式清除钥匙串 Key（保存留空不调用——留空 = 保留已有 Key）。
#[allow(dead_code)]
pub fn clear_embed_api_key() -> Result<(), String> {
    if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER) {
        let _ = entry.delete_credential();
    }
    let _ = std::fs::remove_file(key_file_path());
    Ok(())
}

fn embeddings_url(base_url: &str) -> String {
    format!("{}/embeddings", base_url.trim_end_matches('/'))
}

pub async fn embed_batch(cfg: &EmbedConfig, inputs: &[String]) -> Result<Vec<Vec<f32>>, String> {
    if inputs.is_empty() {
        return Ok(vec![]);
    }
    let mut all = Vec::with_capacity(inputs.len());
    for chunk in inputs.chunks(BATCH) {
        all.extend(embed_once(cfg, chunk).await?);
    }
    Ok(all)
}

async fn embed_once(cfg: &EmbedConfig, inputs: &[String]) -> Result<Vec<Vec<f32>>, String> {
    let url = embeddings_url(&cfg.base_url);
    crate::credentials::validate_endpoint(&url)?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| e.to_string())?;
    let mut req = client.post(&url).json(&serde_json::json!({
        "model": cfg.model,
        "input": inputs,
    }));
    if let Some(k) = cfg.api_key.as_ref().filter(|s| !s.trim().is_empty()) {
        req = req.bearer_auth(k.trim());
    }
    let resp = req
        .send()
        .await
        .map_err(|e| format!("嵌入服务连接失败: {e}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        let detail = serde_json::from_str::<Value>(&body)
            .ok()
            .and_then(|v| v["error"]["message"].as_str().map(|s| s.to_string()))
            .unwrap_or_else(|| {
                if body.len() > 300 {
                    body[..300].to_string()
                } else {
                    body.clone()
                }
            });
        return Err(format!("嵌入服务返回 {status}: {detail}"));
    }
    let body: Value = resp
        .json()
        .await
        .map_err(|e| format!("嵌入响应解析失败: {e}"))?;
    let mut rows: Vec<(usize, Vec<f32>)> = Vec::new();
    let data = body["data"].as_array().ok_or("嵌入响应缺少 data")?;
    for item in data {
        let idx = item["index"].as_u64().unwrap_or(0) as usize;
        let emb = item["embedding"]
            .as_array()
            .ok_or("嵌入响应缺少 embedding")?
            .iter()
            .filter_map(|v| v.as_f64().map(|f| f as f32))
            .collect::<Vec<_>>();
        rows.push((idx, emb));
    }
    rows.sort_by_key(|(i, _)| *i);
    Ok(rows.into_iter().map(|(_, v)| v).collect())
}

pub async fn test_connection(cfg: &EmbedConfig) -> Result<(String, usize), String> {
    let vecs = embed_batch(cfg, &["test".into()]).await?;
    let dim = vecs.first().map(|v| v.len()).unwrap_or(0);
    if dim == 0 {
        return Err("嵌入服务返回空向量".into());
    }
    Ok((cfg.model.clone(), dim))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embed_url_joins_v1() {
        let base = "http://127.0.0.1:11434/v1";
        let url = embeddings_url(base);
        assert_eq!(url, "http://127.0.0.1:11434/v1/embeddings");
    }

    #[test]
    fn embed_url_trims_trailing_slash() {
        let url = embeddings_url("http://127.0.0.1:11434/v1/");
        assert_eq!(url, "http://127.0.0.1:11434/v1/embeddings");
    }

    #[tokio::test]
    async fn embed_batch_empty_returns_ok() {
        let cfg = EmbedConfig {
            base_url: "http://127.0.0.1:11434/v1".into(),
            model: "bge-m3".into(),
            api_key: None,
        };
        let out = embed_batch(&cfg, &[]).await.unwrap();
        assert!(out.is_empty());
    }
}
