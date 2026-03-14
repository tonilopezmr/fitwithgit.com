use chrono::NaiveDate;
use regex::Regex;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue, USER_AGENT};
use serde::Deserialize;
use std::fmt;

// --- Error type ---

#[derive(Debug)]
#[allow(dead_code)]
pub enum SyncError {
    Auth(String),
    Api(String),
    Parse(String),
    MissingCredentials,
}

impl fmt::Display for SyncError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SyncError::Auth(msg) => write!(f, "Auth error: {msg}"),
            SyncError::Api(msg) => write!(f, "API error: {msg}"),
            SyncError::Parse(msg) => write!(f, "Parse error: {msg}"),
            SyncError::MissingCredentials => {
                write!(f, "Missing GARMIN_USERNAME or GARMIN_PASSWORD")
            }
        }
    }
}

impl std::error::Error for SyncError {}

// --- API response types ---

/// Per-day steps data from the batch stats endpoint.
pub struct DailySteps {
    pub date: NaiveDate,
    pub steps: u32,
    pub goal: u32,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GarminActivity {
    pub activity_id: u64,
    pub activity_type: ActivityType,
    pub start_time_local: Option<String>,
    pub duration: Option<f64>,
    pub distance: Option<f64>,
    pub elevation_gain: Option<f64>,
    pub average_speed: Option<f64>,
    pub laps: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityType {
    pub type_key: String,
}

// --- OAuth constants ---

const CONSUMER_KEY: &str = "fc3e99d2-118c-44b8-8ae3-03370dde24c0";
const CONSUMER_SECRET: &str = "E08WAR897WEy2knn7aFBrvegVAf0AFdWBBF";
const SSO_USER_AGENT: &str = "com.garmin.android.apps.connectmobile";
const API_USER_AGENT: &str = "GCM-iOS-5.19.1.2";
const SSO_EMBED_URL: &str = "https://sso.garmin.com/sso/embed";
const API_BASE: &str = "https://connectapi.garmin.com";

// --- GarminSync client ---

pub struct GarminSync {
    access_token: String,
    client: reqwest::Client,
}

impl GarminSync {
    /// Authenticate with Garmin Connect via SSO and obtain an OAuth2 access token.
    pub async fn login(username: &str, password: &str) -> Result<Self, SyncError> {
        let sso_client = reqwest::Client::builder()
            .cookie_store(true)
            .redirect(reqwest::redirect::Policy::default())
            .build()
            .map_err(|e| SyncError::Auth(e.to_string()))?;

        // Step 1: Initialize SSO cookies
        let embed_params = sso_query_params();
        sso_client
            .get(SSO_EMBED_URL)
            .query(&embed_params)
            .header(USER_AGENT, SSO_USER_AGENT)
            .send()
            .await
            .map_err(|e| SyncError::Auth(format!("SSO embed request failed: {e}")))?;

        // Step 2: Get signin page and extract CSRF token
        let signin_url = "https://sso.garmin.com/sso/signin";
        let signin_resp = sso_client
            .get(signin_url)
            .query(&sso_query_params())
            .header(USER_AGENT, SSO_USER_AGENT)
            .header("Referer", SSO_EMBED_URL)
            .send()
            .await
            .map_err(|e| SyncError::Auth(format!("SSO signin page failed: {e}")))?;

        let signin_html = signin_resp
            .text()
            .await
            .map_err(|e| SyncError::Auth(format!("Failed to read signin page: {e}")))?;

        let csrf = extract_csrf(&signin_html)
            .ok_or_else(|| SyncError::Auth("Could not find CSRF token in signin page".into()))?;

        // Step 3: Submit credentials
        let form_params = [
            ("username", username),
            ("password", password),
            ("embed", "true"),
            ("_csrf", &csrf),
        ];

        let login_resp = sso_client
            .post(signin_url)
            .query(&sso_query_params())
            .header(USER_AGENT, SSO_USER_AGENT)
            .header("Referer", signin_url)
            .form(&form_params)
            .send()
            .await
            .map_err(|e| SyncError::Auth(format!("Login POST failed: {e}")))?;

        let login_html = login_resp
            .text()
            .await
            .map_err(|e| SyncError::Auth(format!("Failed to read login response: {e}")))?;

        // Check for success
        let title = extract_title(&login_html).unwrap_or_default();
        if !title.to_lowercase().contains("success") {
            return Err(SyncError::Auth(format!(
                "Login failed. Page title: \"{title}\". Check your credentials."
            )));
        }

        // Step 4: Extract service ticket
        let ticket = extract_ticket(&login_html)
            .ok_or_else(|| SyncError::Auth("Could not find service ticket in response".into()))?;

        // Step 5: Exchange ticket for OAuth1 token
        let preauth_url = format!(
            "{API_BASE}/oauth-service/oauth/preauthorized?ticket={ticket}&login-url={SSO_EMBED_URL}&accepts-mfa-tokens=true"
        );

        let oauth1_resp = oauth1_get(&preauth_url, CONSUMER_KEY, CONSUMER_SECRET, None, None)
            .await
            .map_err(|e| SyncError::Auth(format!("OAuth1 preauthorized failed: {e}")))?;

        let oauth1_params: Vec<(String, String)> = serde_urlencoded::from_str(&oauth1_resp)
            .map_err(|e| SyncError::Auth(format!("Failed to parse OAuth1 response: {e}")))?;

        let oauth_token = find_param(&oauth1_params, "oauth_token")
            .ok_or_else(|| SyncError::Auth("No oauth_token in response".into()))?;
        let oauth_token_secret = find_param(&oauth1_params, "oauth_token_secret")
            .ok_or_else(|| SyncError::Auth("No oauth_token_secret in response".into()))?;

        // Step 6: Exchange OAuth1 for OAuth2 token
        let exchange_url = format!("{API_BASE}/oauth-service/oauth/exchange/user/2.0");

        let oauth2_resp = oauth1_post(
            &exchange_url,
            CONSUMER_KEY,
            CONSUMER_SECRET,
            Some(&oauth_token),
            Some(&oauth_token_secret),
        )
        .await
        .map_err(|e| SyncError::Auth(format!("OAuth2 exchange failed: {e}")))?;

        let oauth2: OAuth2Response = serde_json::from_str(&oauth2_resp).map_err(|e| {
            SyncError::Auth(format!(
                "Failed to parse OAuth2 token: {e} (body: {oauth2_resp})"
            ))
        })?;

        let api_client = reqwest::Client::builder()
            .build()
            .map_err(|e| SyncError::Auth(e.to_string()))?;

        Ok(GarminSync {
            access_token: oauth2.access_token,
            client: api_client,
        })
    }

    fn api_headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static(API_USER_AGENT));
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", self.access_token)).expect("invalid token"),
        );
        headers.insert(
            "DI-Backend",
            HeaderValue::from_static("connectapi.garmin.com"),
        );
        headers.insert("origin", HeaderValue::from_static("https://sso.garmin.com"));
        headers.insert("nk", HeaderValue::from_static("NT"));
        headers
    }

    /// Fetch daily steps in batch using the stats endpoint.
    /// Automatically chunks into 28-day windows (Garmin API limit).
    pub async fn fetch_daily_steps(
        &self,
        start: NaiveDate,
        end: NaiveDate,
    ) -> Result<Vec<DailySteps>, SyncError> {
        use chrono::Duration;
        use std::collections::HashMap;

        let mut all_steps = Vec::new();
        let mut chunk_start = start;

        while chunk_start <= end {
            let chunk_end = (chunk_start + Duration::days(27)).min(end);
            let url = format!(
                "{API_BASE}/usersummary-service/stats/steps/daily/{}/{}",
                chunk_start.format("%Y-%m-%d"),
                chunk_end.format("%Y-%m-%d")
            );

            let resp = self
                .client
                .get(&url)
                .headers(self.api_headers())
                .send()
                .await
                .map_err(|e| SyncError::Api(format!("Steps stats request failed: {e}")))?;

            if resp.status() == reqwest::StatusCode::NO_CONTENT {
                chunk_start = chunk_end + Duration::days(1);
                continue;
            }

            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                eprintln!("Warning: Steps stats returned {status}: {body}");
                chunk_start = chunk_end + Duration::days(1);
                continue;
            }

            let body: serde_json::Value = resp
                .json()
                .await
                .map_err(|e| SyncError::Parse(format!("Failed to parse steps stats: {e}")))?;

            // Handle two possible response formats:
            // 1. Flat array: [{calendarDate, totalSteps, stepGoal}, ...]
            // 2. Nested: {allMetrics: {metricsMap: {WELLNESS_TOTAL_STEPS: [...]}}}
            if let Some(arr) = body.as_array() {
                // Flat array format
                for entry in arr {
                    let date_str = entry.get("calendarDate").and_then(|v| v.as_str());
                    let steps = entry
                        .get("totalSteps")
                        .and_then(|v| v.as_u64())
                        .or_else(|| {
                            entry
                                .get("value")
                                .and_then(|v| v.as_f64())
                                .map(|f| f as u64)
                        });
                    let goal = entry
                        .get("stepGoal")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(10000);

                    if let (Some(ds), Some(s)) = (date_str, steps)
                        && s > 0
                        && let Ok(date) = NaiveDate::parse_from_str(ds, "%Y-%m-%d")
                    {
                        all_steps.push(DailySteps {
                            date,
                            steps: s as u32,
                            goal: goal as u32,
                        });
                    }
                }
            } else {
                // Nested metrics format
                let metrics_map = body
                    .get("allMetrics")
                    .and_then(|am| am.get("metricsMap"))
                    .unwrap_or(&body);

                let steps_array = metrics_map
                    .get("WELLNESS_TOTAL_STEPS")
                    .and_then(|v| v.as_array());
                let goals_array = metrics_map
                    .get("WELLNESS_TOTAL_STEP_GOAL")
                    .and_then(|v| v.as_array());

                let mut goal_map: HashMap<String, u32> = HashMap::new();
                if let Some(goals) = goals_array {
                    for entry in goals {
                        if let (Some(ds), Some(value)) = (
                            entry.get("calendarDate").and_then(|v| v.as_str()),
                            entry.get("value").and_then(|v| v.as_f64()),
                        ) && value > 0.0
                        {
                            goal_map.insert(ds.to_string(), value as u32);
                        }
                    }
                }

                if let Some(steps) = steps_array {
                    for entry in steps {
                        if let (Some(ds), Some(value)) = (
                            entry.get("calendarDate").and_then(|v| v.as_str()),
                            entry.get("value").and_then(|v| v.as_f64()),
                        ) {
                            let steps_count = value as u32;
                            if steps_count > 0
                                && let Ok(date) = NaiveDate::parse_from_str(ds, "%Y-%m-%d")
                            {
                                let goal = goal_map.get(ds).copied().unwrap_or(10000);
                                all_steps.push(DailySteps {
                                    date,
                                    steps: steps_count,
                                    goal,
                                });
                            }
                        }
                    }
                }
            }

            chunk_start = chunk_end + Duration::days(1);
        }

        Ok(all_steps)
    }

    /// Fetch activities within a date range.
    pub async fn fetch_activities(
        &self,
        start: NaiveDate,
        end: NaiveDate,
    ) -> Result<Vec<GarminActivity>, SyncError> {
        let mut all_activities = Vec::new();
        let mut offset = 0u32;
        let limit = 100u32;

        loop {
            let url = format!("{API_BASE}/activitylist-service/activities/search/activities");
            let resp = self
                .client
                .get(&url)
                .headers(self.api_headers())
                .query(&[
                    ("startDate", start.format("%Y-%m-%d").to_string()),
                    ("endDate", end.format("%Y-%m-%d").to_string()),
                    ("start", offset.to_string()),
                    ("limit", limit.to_string()),
                ])
                .send()
                .await
                .map_err(|e| SyncError::Api(format!("Activities request failed: {e}")))?;

            if resp.status() == reqwest::StatusCode::NO_CONTENT {
                break;
            }

            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                return Err(SyncError::Api(format!(
                    "Activities returned {status}: {body}"
                )));
            }

            let batch: Vec<GarminActivity> = resp
                .json()
                .await
                .map_err(|e| SyncError::Parse(format!("Failed to parse activities: {e}")))?;

            let count = batch.len();
            all_activities.extend(batch);

            if (count as u32) < limit {
                break;
            }
            offset += limit;
        }

        Ok(all_activities)
    }
}

// --- Helper functions ---

fn sso_query_params() -> Vec<(&'static str, &'static str)> {
    vec![
        ("id", "gauth-widget"),
        ("embedWidget", "true"),
        ("gauthHost", SSO_EMBED_URL),
        ("service", SSO_EMBED_URL),
        ("source", SSO_EMBED_URL),
        ("redirectAfterAccountLoginUrl", SSO_EMBED_URL),
        ("redirectAfterAccountCreationUrl", SSO_EMBED_URL),
    ]
}

fn extract_csrf(html: &str) -> Option<String> {
    let re = Regex::new(r#"name="_csrf"\s+value="([^"]+)""#).ok()?;
    re.captures(html).map(|c| c[1].to_string())
}

fn extract_title(html: &str) -> Option<String> {
    let re = Regex::new(r"<title>([^<]+)</title>").ok()?;
    re.captures(html).map(|c| c[1].to_string())
}

fn extract_ticket(html: &str) -> Option<String> {
    let re = Regex::new(r#"embed\?ticket=([^"]+)""#).ok()?;
    re.captures(html).map(|c| c[1].to_string())
}

fn find_param(params: &[(String, String)], key: &str) -> Option<String> {
    params
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.clone())
}

#[derive(Deserialize)]
struct OAuth2Response {
    access_token: String,
}

/// Build OAuth1 Authorization header using HMAC-SHA1 signing.
fn build_oauth1_header(
    method: &str,
    url: &str,
    consumer_key: &str,
    consumer_secret: &str,
    token: Option<&str>,
    token_secret: Option<&str>,
) -> String {
    use hmac::{Hmac, Mac};
    use sha1::Sha1;

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        .to_string();
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let nonce: String = (0..32u128)
        .map(|i| {
            // Simple xorshift-style mixing to get different chars per position
            let mut x = seed.wrapping_add(i).wrapping_mul(6364136223846793005);
            x ^= x >> 17;
            let idx = (x % 36) as usize;
            b"abcdefghijklmnopqrstuvwxyz0123456789"[idx] as char
        })
        .collect();

    // Parse URL to separate base URL and query params
    let (base_url, query_params) = if let Some(q_pos) = url.find('?') {
        (&url[..q_pos], Some(&url[q_pos + 1..]))
    } else {
        (url, None)
    };

    let mut params: Vec<(String, String)> = vec![
        ("oauth_consumer_key".to_string(), consumer_key.to_string()),
        ("oauth_nonce".to_string(), nonce.clone()),
        (
            "oauth_signature_method".to_string(),
            "HMAC-SHA1".to_string(),
        ),
        ("oauth_timestamp".to_string(), timestamp.clone()),
        ("oauth_version".to_string(), "1.0".to_string()),
    ];

    if let Some(t) = token {
        params.push(("oauth_token".to_string(), t.to_string()));
    }

    // Include query parameters in signing
    if let Some(qp) = query_params {
        for pair in qp.split('&') {
            if let Some((k, v)) = pair.split_once('=') {
                params.push((k.to_string(), v.to_string()));
            }
        }
    }

    params.sort();

    let param_string: String = params
        .iter()
        .map(|(k, v)| format!("{}={}", percent_encode(k), percent_encode(v)))
        .collect::<Vec<_>>()
        .join("&");

    let base_string = format!(
        "{}&{}&{}",
        method.to_uppercase(),
        percent_encode(base_url),
        percent_encode(&param_string)
    );

    let signing_key = format!(
        "{}&{}",
        percent_encode(consumer_secret),
        percent_encode(token_secret.unwrap_or(""))
    );

    type HmacSha1 = Hmac<Sha1>;
    let mut mac = HmacSha1::new_from_slice(signing_key.as_bytes()).unwrap();
    mac.update(base_string.as_bytes());
    let signature = base64_encode(&mac.finalize().into_bytes());

    let mut header_params = vec![
        format!("oauth_consumer_key=\"{}\"", percent_encode(consumer_key)),
        format!("oauth_nonce=\"{}\"", percent_encode(&nonce)),
        format!("oauth_signature=\"{}\"", percent_encode(&signature)),
        "oauth_signature_method=\"HMAC-SHA1\"".to_string(),
        format!("oauth_timestamp=\"{}\"", timestamp),
        "oauth_version=\"1.0\"".to_string(),
    ];

    if let Some(t) = token {
        header_params.push(format!("oauth_token=\"{}\"", percent_encode(t)));
    }

    format!("OAuth {}", header_params.join(", "))
}

fn percent_encode(s: &str) -> String {
    let mut result = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b == b'-' || b == b'.' || b == b'_' || b == b'~' {
            result.push(b as char);
        } else {
            result.push_str(&format!("%{:02X}", b));
        }
    }
    result
}

fn base64_encode(data: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(data)
}

/// Perform an OAuth1-signed GET request.
async fn oauth1_get(
    url: &str,
    consumer_key: &str,
    consumer_secret: &str,
    token: Option<&str>,
    token_secret: Option<&str>,
) -> Result<String, String> {
    let auth = build_oauth1_header(
        "GET",
        url,
        consumer_key,
        consumer_secret,
        token,
        token_secret,
    );
    let client = reqwest::Client::new();
    let resp = client
        .get(url)
        .header(USER_AGENT, SSO_USER_AGENT)
        .header(AUTHORIZATION, &auth)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    resp.text().await.map_err(|e| e.to_string())
}

/// Perform an OAuth1-signed POST request.
async fn oauth1_post(
    url: &str,
    consumer_key: &str,
    consumer_secret: &str,
    token: Option<&str>,
    token_secret: Option<&str>,
) -> Result<String, String> {
    let auth = build_oauth1_header(
        "POST",
        url,
        consumer_key,
        consumer_secret,
        token,
        token_secret,
    );
    let client = reqwest::Client::new();
    let resp = client
        .post(url)
        .header(USER_AGENT, SSO_USER_AGENT)
        .header(AUTHORIZATION, &auth)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Content-Length", "0")
        .send()
        .await
        .map_err(|e| e.to_string())?;

    resp.text().await.map_err(|e| e.to_string())
}
