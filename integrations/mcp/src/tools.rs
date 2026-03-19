use std::path::PathBuf;

use chrono::NaiveDate;
use rmcp::{
    ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, Content, ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router,
};
use serde_json::json;
use tokio::process::Command;

use crate::data;

// --- Parameter structs ---

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ReadFitLogParams {
    #[schemars(
        description = "Filter by activity code (S=Steps, R=Run, W=Swim, B=Bike, G=Gym, X=Stretch, K=Ski, H=Hike, Z=Sleep, V=Recovery)"
    )]
    pub activity: Option<String>,
    #[schemars(description = "Start date filter (YYYY-MM-DD)")]
    pub since: Option<String>,
    #[schemars(description = "End date filter (YYYY-MM-DD)")]
    pub until: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct AddEntryParams {
    #[schemars(
        description = "fit.log entry line, e.g. 'G,260319,1' or 'R,260319,32,5.3,6.0'. Format: <code>,<YYMMDD>,<fields...>"
    )]
    pub entry: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SyncParams {
    #[schemars(description = "Start date (YYYY-MM-DD)")]
    pub since: Option<String>,
    #[schemars(description = "End date (YYYY-MM-DD)")]
    pub until: Option<String>,
    #[schemars(description = "Preview changes without writing to fit.log")]
    pub dry_run: Option<bool>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct GetSummaryParams {
    #[schemars(description = "Start date (YYYY-MM-DD)")]
    pub since: Option<String>,
    #[schemars(description = "End date (YYYY-MM-DD)")]
    pub until: Option<String>,
}

// --- MCP Server ---

#[derive(Debug, Clone)]
pub struct FitMcp {
    fit_log_path: PathBuf,
    garmin_bin: String,
    whoop_bin: String,
    tool_router: ToolRouter<Self>,
}

fn parse_ymd(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

#[tool_router]
impl FitMcp {
    pub fn new(fit_log_path: PathBuf, garmin_bin: String, whoop_bin: String) -> Self {
        Self {
            fit_log_path,
            garmin_bin,
            whoop_bin,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "Read the fit.log fitness data file. Returns raw CSV lines. Each line: <code>,<YYMMDD>,<fields>. Codes: S=Steps, R=Run, W=Swim, B=Bike, G=Gym, X=Stretch, K=Ski, H=Hike, Z=Sleep, V=Recovery."
    )]
    fn read_fit_log(
        &self,
        Parameters(params): Parameters<ReadFitLogParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let content = std::fs::read_to_string(&self.fit_log_path).unwrap_or_default();
        let records = data::parse_content(&content);

        let since = params.since.as_deref().and_then(parse_ymd);
        let until = params.until.as_deref().and_then(parse_ymd);

        let mut lines: Vec<String> = Vec::new();
        for record in &records {
            if let Some(code) = &params.activity
                && data::activity_code(&record.activity) != code.as_str()
            {
                continue;
            }
            if let Some(s) = since
                && record.date < s
            {
                continue;
            }
            if let Some(u) = until
                && record.date > u
            {
                continue;
            }
            lines.push(data::format_record(record));
        }

        if lines.is_empty() {
            Ok(CallToolResult::success(vec![Content::text(
                "No matching entries found.",
            )]))
        } else {
            Ok(CallToolResult::success(vec![Content::text(
                lines.join("\n"),
            )]))
        }
    }

    #[tool(
        description = "Add a manual entry to fit.log. Entry format: <code>,<YYMMDD>,<fields>. Example: 'G,260319,1' (gym session), 'R,260319,32,5.3,6.0' (run). The entry is validated before writing."
    )]
    fn add_entry(
        &self,
        Parameters(params): Parameters<AddEntryParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let entry = params.entry.trim().to_string();

        // Validate the entry parses correctly
        if data::parse_line(&entry).is_none() {
            return Ok(CallToolResult::error(vec![Content::text(format!(
                "Invalid entry: '{entry}'. Expected format: <code>,<YYMMDD>,<fields>. Example: G,260319,1"
            ))]));
        }

        // Append to fit.log
        use std::io::Write;
        let mut file = match std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.fit_log_path)
        {
            Ok(f) => f,
            Err(e) => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Failed to open fit.log: {e}"
                ))]));
            }
        };

        if let Err(e) = writeln!(file, "{entry}") {
            return Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to write entry: {e}"
            ))]));
        }

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Added: {entry}"
        ))]))
    }

    #[tool(
        description = "Sync fitness data from Garmin Connect to fit.log. Fetches steps and activities (runs, gym, etc.). Requires GARMIN_USERNAME and GARMIN_PASSWORD environment variables."
    )]
    async fn garmin_sync(
        &self,
        Parameters(params): Parameters<SyncParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let mut args = vec![
            "--fit-log".to_string(),
            self.fit_log_path.to_string_lossy().to_string(),
        ];
        if let Some(since) = &params.since {
            args.push("--since".to_string());
            args.push(since.clone());
        }
        if let Some(until) = &params.until {
            args.push("--until".to_string());
            args.push(until.clone());
        }
        if params.dry_run.unwrap_or(false) {
            args.push("--dry-run".to_string());
        }

        run_sync_command(&self.garmin_bin, &args).await
    }

    #[tool(
        description = "Sync fitness data from Whoop to fit.log. Fetches workouts, sleep, and recovery metrics. Requires WHOOP_CLIENT_ID and WHOOP_CLIENT_SECRET environment variables. First run requires browser-based OAuth authorization."
    )]
    async fn whoop_sync(
        &self,
        Parameters(params): Parameters<SyncParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let mut args = vec![
            "--fit-log".to_string(),
            self.fit_log_path.to_string_lossy().to_string(),
        ];
        if let Some(since) = &params.since {
            args.push("--since".to_string());
            args.push(since.clone());
        }
        if let Some(until) = &params.until {
            args.push("--until".to_string());
            args.push(until.clone());
        }
        if params.dry_run.unwrap_or(false) {
            args.push("--dry-run".to_string());
        }

        run_sync_command(&self.whoop_bin, &args).await
    }

    #[tool(
        description = "Get a fitness summary from fit.log. Returns activity counts and statistics for the given date range."
    )]
    fn get_summary(
        &self,
        Parameters(params): Parameters<GetSummaryParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let records = data::parse_file(&self.fit_log_path);
        let since = params.since.as_deref().and_then(parse_ymd);
        let until = params.until.as_deref().and_then(parse_ymd);

        let filtered: Vec<&data::ActivityRecord> = records
            .iter()
            .filter(|r| {
                if let Some(s) = since
                    && r.date < s
                {
                    return false;
                }
                if let Some(u) = until
                    && r.date > u
                {
                    return false;
                }
                true
            })
            .collect();

        let mut activity_counts: std::collections::BTreeMap<String, u32> =
            std::collections::BTreeMap::new();
        let mut total_run_km: f32 = 0.0;
        let mut total_run_min: u32 = 0;
        let mut total_bike_km: f32 = 0.0;
        let mut total_hike_km: f32 = 0.0;
        let mut sleep_scores: Vec<u8> = Vec::new();
        let mut recovery_pcts: Vec<u8> = Vec::new();
        let mut step_totals: Vec<u32> = Vec::new();

        for r in &filtered {
            let code = data::activity_code(&r.activity).to_string();
            *activity_counts.entry(code).or_insert(0) += 1;

            match &r.activity {
                data::Activity::Run {
                    duration,
                    distance_km,
                    ..
                } => {
                    total_run_km += distance_km;
                    total_run_min += *duration as u32;
                }
                data::Activity::Bike { distance_km, .. } => {
                    total_bike_km += distance_km;
                }
                data::Activity::Hike { distance_km, .. } => {
                    total_hike_km += distance_km;
                }
                data::Activity::Sleep { score, .. } => {
                    sleep_scores.push(*score);
                }
                data::Activity::Recovery { recovery_pct, .. } => {
                    recovery_pcts.push(*recovery_pct);
                }
                data::Activity::Steps { steps, .. } => {
                    step_totals.push(*steps);
                }
                _ => {}
            }
        }

        let mut summary = json!({
            "total_records": filtered.len(),
            "activity_counts": activity_counts,
        });

        let obj = summary.as_object_mut().unwrap();
        if total_run_km > 0.0 {
            obj.insert(
                "running".to_string(),
                json!({
                    "total_km": format!("{total_run_km:.1}"),
                    "total_min": total_run_min,
                }),
            );
        }
        if total_bike_km > 0.0 {
            obj.insert(
                "cycling".to_string(),
                json!({ "total_km": format!("{total_bike_km:.1}") }),
            );
        }
        if total_hike_km > 0.0 {
            obj.insert(
                "hiking".to_string(),
                json!({ "total_km": format!("{total_hike_km:.1}") }),
            );
        }
        if !step_totals.is_empty() {
            let avg = step_totals.iter().sum::<u32>() / step_totals.len() as u32;
            obj.insert("avg_daily_steps".to_string(), json!(avg));
        }
        if !sleep_scores.is_empty() {
            let avg =
                sleep_scores.iter().map(|s| *s as u32).sum::<u32>() / sleep_scores.len() as u32;
            obj.insert("avg_sleep_score".to_string(), json!(avg));
        }
        if !recovery_pcts.is_empty() {
            let avg =
                recovery_pcts.iter().map(|p| *p as u32).sum::<u32>() / recovery_pcts.len() as u32;
            obj.insert("avg_recovery_pct".to_string(), json!(avg));
        }

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&summary).unwrap(),
        )]))
    }
}

async fn run_sync_command(bin: &str, args: &[String]) -> Result<CallToolResult, rmcp::ErrorData> {
    let output = match Command::new(bin).args(args).output().await {
        Ok(o) => o,
        Err(e) => {
            return Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to run {bin}: {e}"
            ))]));
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let mut result = String::new();
    if !stdout.is_empty() {
        result.push_str(&stdout);
    }
    if !stderr.is_empty() {
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str("[stderr] ");
        result.push_str(&stderr);
    }
    if result.is_empty() {
        result.push_str("Command completed with no output.");
    }

    if output.status.success() {
        Ok(CallToolResult::success(vec![Content::text(result)]))
    } else {
        Ok(CallToolResult::error(vec![Content::text(result)]))
    }
}

#[tool_handler]
impl ServerHandler for FitMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("Fitness tracking MCP server for fit.log. Read fitness data, add entries, sync from Garmin/Whoop, and get summaries.".to_string())
    }
}
