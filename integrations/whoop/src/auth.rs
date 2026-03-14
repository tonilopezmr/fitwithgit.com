use axum::{Router, extract::Query, routing::get};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::sync::oneshot;

const AUTH_URL: &str = "https://api.prod.whoop.com/oauth/oauth2/auth";
const TOKEN_URL: &str = "https://api.prod.whoop.com/oauth/oauth2/token";
const REDIRECT_PORT: u16 = 9876;
const SCOPES: &str = "read:recovery read:sleep read:workout read:cycles offline";

#[derive(Serialize, Deserialize)]
pub struct TokenStore {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
}

impl TokenStore {
    fn path() -> PathBuf {
        dirs().join("fitwithgit").join("whoop_tokens.json")
    }

    pub fn load() -> Option<Self> {
        let data = std::fs::read_to_string(Self::path()).ok()?;
        serde_json::from_str(&data).ok()
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create config directory: {e}"))?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize tokens: {e}"))?;
        std::fs::write(&path, json).map_err(|e| format!("Failed to write token file: {e}"))?;
        Ok(())
    }

    pub fn is_expired(&self) -> bool {
        let now = chrono::Utc::now().timestamp();
        self.expires_at <= now
    }
}

fn dirs() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        PathBuf::from(xdg)
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".config")
    } else {
        PathBuf::from(".config")
    }
}

#[derive(Deserialize)]
struct CallbackParams {
    code: String,
    state: String,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: i64,
}

fn generate_state() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    format!("{:x}", nanos)
}

pub async fn authorize(client_id: &str, client_secret: &str) -> Result<TokenStore, String> {
    let redirect_uri = format!("http://127.0.0.1:{REDIRECT_PORT}/callback");
    let state = generate_state();

    let auth_url = format!(
        "{AUTH_URL}?client_id={client_id}&redirect_uri={redirect_uri}&response_type=code&scope={SCOPES}&state={state}",
    );

    println!("Opening browser for Whoop authorization...");
    println!("If the browser doesn't open, visit:\n{auth_url}");

    // Try to open browser
    let _ = std::process::Command::new("open").arg(&auth_url).spawn();

    // Start callback server
    let (tx, rx) = oneshot::channel::<(String, String)>();
    let tx = std::sync::Arc::new(std::sync::Mutex::new(Some(tx)));

    let callback_handler = {
        let tx = tx.clone();
        move |Query(params): Query<CallbackParams>| {
            let tx = tx.clone();
            async move {
                if let Ok(mut guard) = tx.lock()
                    && let Some(sender) = guard.take()
                {
                    let _ = sender.send((params.code, params.state));
                }
                axum::response::Html(
                    "<html><body><h2>Authorization successful!</h2><p>You can close this tab and return to the terminal.</p></body></html>",
                )
            }
        }
    };

    let app = Router::new().route("/callback", get(callback_handler));
    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{REDIRECT_PORT}"))
        .await
        .map_err(|e| format!("Failed to bind callback server on port {REDIRECT_PORT}: {e}"))?;

    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.ok();
    });

    // Wait for callback with timeout
    let (code, received_state) = tokio::time::timeout(std::time::Duration::from_secs(120), rx)
        .await
        .map_err(|_| "Authorization timed out after 120 seconds".to_string())?
        .map_err(|_| "Callback channel closed unexpectedly".to_string())?;

    server.abort();

    if received_state != state {
        return Err("CSRF state mismatch — possible attack".to_string());
    }

    // Exchange code for tokens
    exchange_code(client_id, client_secret, &code, &redirect_uri).await
}

async fn exchange_code(
    client_id: &str,
    client_secret: &str,
    code: &str,
    redirect_uri: &str,
) -> Result<TokenStore, String> {
    let client = reqwest::Client::new();
    let response = client
        .post(TOKEN_URL)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("client_id", client_id),
            ("client_secret", client_secret),
        ])
        .send()
        .await
        .map_err(|e| format!("Token exchange request failed: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Token exchange failed ({status}): {body}"));
    }

    let token_resp: TokenResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse token response: {e}"))?;

    let store = TokenStore {
        access_token: token_resp.access_token,
        refresh_token: token_resp.refresh_token.unwrap_or_default(),
        expires_at: chrono::Utc::now().timestamp() + token_resp.expires_in,
    };
    store.save()?;
    Ok(store)
}

pub async fn refresh_token(
    client_id: &str,
    client_secret: &str,
    refresh: &str,
) -> Result<TokenStore, String> {
    let client = reqwest::Client::new();
    let response = client
        .post(TOKEN_URL)
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh),
            ("client_id", client_id),
            ("client_secret", client_secret),
        ])
        .send()
        .await
        .map_err(|e| format!("Token refresh request failed: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Token refresh failed ({status}): {body}"));
    }

    let token_resp: TokenResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse refresh response: {e}"))?;

    let store = TokenStore {
        access_token: token_resp.access_token,
        refresh_token: token_resp
            .refresh_token
            .unwrap_or_else(|| refresh.to_string()),
        expires_at: chrono::Utc::now().timestamp() + token_resp.expires_in,
    };
    store.save()?;
    Ok(store)
}
