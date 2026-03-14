use chrono::NaiveDate;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use std::fmt;

const BASE_URL: &str = "https://api.prod.whoop.com/developer";

// --- Error type ---

pub enum SyncError {
    Auth(String),
    Api(String),
    Parse(String),
}

impl fmt::Display for SyncError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SyncError::Auth(msg) => write!(f, "Auth error: {msg}"),
            SyncError::Api(msg) => write!(f, "API error: {msg}"),
            SyncError::Parse(msg) => write!(f, "Parse error: {msg}"),
        }
    }
}

// --- API response types ---

#[derive(Deserialize)]
pub struct PaginatedResponse<T> {
    pub records: Vec<T>,
    pub next_token: Option<String>,
}

#[derive(Deserialize)]
pub struct WhoopWorkout {
    #[allow(dead_code)]
    pub id: serde_json::Value,
    pub sport_id: Option<u32>,
    pub sport_name: Option<String>,
    pub start: String,
    pub end: String,
    #[allow(dead_code)]
    pub score_state: Option<String>,
    pub score: Option<WorkoutScore>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
pub struct WorkoutScore {
    pub strain: Option<f64>,
    pub average_heart_rate: Option<u16>,
    pub max_heart_rate: Option<u16>,
    pub kilojoule: Option<f64>,
    pub distance_meter: Option<f64>,
    pub altitude_gain_meter: Option<f64>,
    pub zone_duration: Option<ZoneDuration>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
pub struct ZoneDuration {
    pub zone_zero_milli: Option<u64>,
    pub zone_one_milli: Option<u64>,
    pub zone_two_milli: Option<u64>,
    pub zone_three_milli: Option<u64>,
    pub zone_four_milli: Option<u64>,
    pub zone_five_milli: Option<u64>,
}

#[derive(Deserialize)]
pub struct WhoopSleep {
    #[allow(dead_code)]
    pub id: serde_json::Value,
    #[allow(dead_code)]
    pub start: String,
    pub end: String,
    pub nap: Option<bool>,
    pub score_state: Option<String>,
    pub score: Option<SleepScore>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
pub struct SleepScore {
    pub stage_summary: Option<StageSummary>,
    pub sleep_performance_percentage: Option<f64>,
    pub respiratory_rate: Option<f64>,
    pub sleep_consistency_percentage: Option<f64>,
    pub sleep_efficiency_percentage: Option<f64>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
pub struct StageSummary {
    pub total_in_bed_time_milli: Option<u64>,
    pub total_awake_time_milli: Option<u64>,
    pub total_light_sleep_time_milli: Option<u64>,
    pub total_slow_wave_sleep_time_milli: Option<u64>,
    pub total_rem_sleep_time_milli: Option<u64>,
    pub sleep_cycle_count: Option<u32>,
    pub disturbance_count: Option<u32>,
}

#[derive(Deserialize)]
pub struct WhoopRecovery {
    #[allow(dead_code)]
    pub cycle_id: Option<serde_json::Value>,
    #[allow(dead_code)]
    pub sleep_id: Option<serde_json::Value>,
    pub created_at: Option<String>,
    pub score_state: Option<String>,
    pub score: Option<RecoveryScoreData>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
pub struct RecoveryScoreData {
    pub recovery_score: Option<f64>,
    pub resting_heart_rate: Option<f64>,
    pub hrv_rmssd_milli: Option<f64>,
    pub spo2_percentage: Option<f64>,
    pub skin_temp_celsius: Option<f64>,
}

// --- Client ---

pub struct WhoopClient {
    access_token: String,
    client: reqwest::Client,
}

impl WhoopClient {
    pub fn new(access_token: String) -> Self {
        Self {
            access_token,
            client: reqwest::Client::new(),
        }
    }

    async fn paginated_get<T: DeserializeOwned>(
        &self,
        path: &str,
        start: NaiveDate,
        end: NaiveDate,
    ) -> Result<Vec<T>, SyncError> {
        let start_str = format!("{start}T00:00:00.000Z");
        let end_str = format!("{end}T23:59:59.999Z");
        let mut all_records = Vec::new();
        let mut next_token: Option<String> = None;

        loop {
            let url = format!("{BASE_URL}{path}");
            let mut request = self
                .client
                .get(&url)
                .bearer_auth(&self.access_token)
                .query(&[
                    ("start", start_str.as_str()),
                    ("end", end_str.as_str()),
                    ("limit", "25"),
                ]);

            if let Some(token) = &next_token {
                request = request.query(&[("nextToken", token.as_str())]);
            }

            let response = request
                .send()
                .await
                .map_err(|e| SyncError::Api(format!("Request to {path} failed: {e}")))?;

            let status = response.status();
            if status == reqwest::StatusCode::UNAUTHORIZED {
                return Err(SyncError::Auth("Access token expired or invalid".into()));
            }
            if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                return Err(SyncError::Api("Rate limited by Whoop API".into()));
            }
            if !status.is_success() {
                let body = response.text().await.unwrap_or_default();
                return Err(SyncError::Api(format!("{path} returned {status}: {body}")));
            }

            let page: PaginatedResponse<T> = response
                .json()
                .await
                .map_err(|e| SyncError::Parse(format!("Failed to parse {path} response: {e}")))?;

            all_records.extend(page.records);

            match page.next_token {
                Some(token) if !token.is_empty() => next_token = Some(token),
                _ => break,
            }
        }

        Ok(all_records)
    }

    pub async fn fetch_workouts(
        &self,
        start: NaiveDate,
        end: NaiveDate,
    ) -> Result<Vec<WhoopWorkout>, SyncError> {
        self.paginated_get("/v2/activity/workout", start, end).await
    }

    pub async fn fetch_sleep(
        &self,
        start: NaiveDate,
        end: NaiveDate,
    ) -> Result<Vec<WhoopSleep>, SyncError> {
        self.paginated_get("/v2/activity/sleep", start, end).await
    }

    pub async fn fetch_recovery(
        &self,
        start: NaiveDate,
        end: NaiveDate,
    ) -> Result<Vec<WhoopRecovery>, SyncError> {
        self.paginated_get("/v2/recovery", start, end).await
    }
}
