// ollama/client.rs — Streaming HTTP vers l'API Ollama.

use serde_json::Value;
use tokio::sync::mpsc;

#[derive(Debug)]
pub enum OllamaMsg {
    Token(String),
    Done,
    Error(String),
}

/// Client HTTP réutilisable (connexions keep-alive, pool de connexions).
static CLIENT: std::sync::LazyLock<reqwest::Client> =
    std::sync::LazyLock::new(|| reqwest::Client::new());

/// Vérifie que l'URL de base pointe vers localhost pour éviter les fuites de données.
/// Extrait le hostname et le valide strictement (localhost, 127.0.0.1, ::1).
fn validate_local_url(base_url: &str) -> Result<(), String> {
    // Retirer le préfixe de schéma
    let without_scheme = base_url
        .strip_prefix("http://")
        .or_else(|| base_url.strip_prefix("https://"))
        .ok_or_else(|| {
            format!("URL Ollama invalide (doit commencer par http://): {base_url}")
        })?;

    // Extraire le host seul : avant le premier '/' (chemin) ou ':' (port)
    let authority = without_scheme.split('/').next().unwrap_or("");
    // Gérer IPv6 entre crochets "[::1]:port"
    let host = if authority.starts_with('[') {
        authority
            .split(']')
            .next()
            .map(|s| s.trim_start_matches('['))
            .unwrap_or("")
    } else {
        authority.split(':').next().unwrap_or("")
    };

    let is_local = matches!(
        host.to_ascii_lowercase().as_str(),
        "localhost" | "127.0.0.1" | "::1"
    );

    if is_local {
        Ok(())
    } else {
        Err(format!(
            "URL Ollama refusée (doit pointer vers localhost) : {base_url}"
        ))
    }
}

/// Lance une requête streaming vers `POST /api/chat`.
/// Les tokens sont envoyés via `tx` au fil de l'eau.
pub async fn stream_chat(
    base_url: &str,
    model: &str,
    messages: Vec<Value>,
    tx: mpsc::Sender<OllamaMsg>,
) {
    if let Err(e) = validate_local_url(base_url) {
        let _ = tx.send(OllamaMsg::Error(e)).await;
        return;
    }

    let url = format!("{base_url}/api/chat");
    let body = serde_json::json!({
        "model": model,
        "messages": messages,
        "stream": true
    });

    let resp = match CLIENT.post(&url).json(&body).send().await {
        Ok(r) if r.status().is_success() => r,
        Ok(r) => {
            let _ = tx.send(OllamaMsg::Error(format!("HTTP {}", r.status()))).await;
            return;
        }
        Err(e) => {
            let _ = tx.send(OllamaMsg::Error(e.to_string())).await;
            return;
        }
    };

    use tokio::io::{AsyncBufReadExt, BufReader};
    use tokio_util::io::StreamReader;
    use futures_util::StreamExt;

    let stream = resp.bytes_stream();
    // Convertir en AsyncRead compatible tokio
    let mapped = stream.map(|r| {
        r.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
    });
    let reader = BufReader::new(StreamReader::new(mapped));
    let mut lines = reader.lines();

    while let Ok(Some(line)) = lines.next_line().await {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }
        if let Ok(parsed) = serde_json::from_str::<Value>(&line) {
            if let Some(token) = parsed
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(Value::as_str)
            {
                if !token.is_empty() {
                    let _ = tx.send(OllamaMsg::Token(token.to_string())).await;
                }
            }
            if parsed.get("done").and_then(Value::as_bool).unwrap_or(false) {
                break;
            }
        }
    }

    let _ = tx.send(OllamaMsg::Done).await;
}

/// Récupère les noms des modèles disponibles via `GET /api/tags`.
pub async fn list_models(base_url: &str) -> Result<Vec<String>, String> {
    validate_local_url(base_url)?;

    let url = format!("{base_url}/api/tags");
    let resp = CLIENT
        .get(&url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json::<Value>()
        .await
        .map_err(|e| e.to_string())?;

    let names = resp
        .get("models")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m.get("name").and_then(Value::as_str).map(String::from))
                .collect()
        })
        .unwrap_or_default();
    Ok(names)
}
