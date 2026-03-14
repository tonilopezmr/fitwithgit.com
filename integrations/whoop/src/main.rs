mod auth;
mod client;
mod mapping;

use chrono::{Duration, Local, NaiveDate};
use clap::Parser;
use std::collections::HashSet;
use std::io::Write;

#[derive(Parser)]
#[command(name = "whoop-sync", about = "Sync Whoop data to fit.log")]
struct Args {
    /// Path to fit.log file
    #[arg(long, default_value = "./fit.log")]
    fit_log: String,

    /// Start date (YYYY-MM-DD). Default: day after last fit.log entry.
    #[arg(long)]
    since: Option<NaiveDate>,

    /// End date (YYYY-MM-DD). Default: today.
    #[arg(long)]
    until: Option<NaiveDate>,

    /// Preview what would be synced without writing
    #[arg(long)]
    dry_run: bool,

    /// Force re-authentication even if tokens exist
    #[arg(long)]
    reauth: bool,
}

fn parse_fit_date(line: &str) -> Option<NaiveDate> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let parts: Vec<&str> = line.split(',').collect();
    if parts.len() < 2 || parts[1].len() != 6 {
        return None;
    }
    let s = parts[1];
    let y = 2000 + s[0..2].parse::<i32>().ok()?;
    let m = s[2..4].parse::<u32>().ok()?;
    let d = s[4..6].parse::<u32>().ok()?;
    NaiveDate::from_ymd_opt(y, m, d)
}

fn read_existing(path: &str) -> (Vec<String>, Option<NaiveDate>) {
    let content = std::fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
    let last_date = lines.iter().filter_map(|l| parse_fit_date(l)).max();
    (lines, last_date)
}

fn existing_fingerprints(lines: &[String]) -> HashSet<String> {
    lines
        .iter()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect()
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    // Load OAuth2 credentials
    let client_id = std::env::var("WHOOP_CLIENT_ID").unwrap_or_default();
    let client_secret = std::env::var("WHOOP_CLIENT_SECRET").unwrap_or_default();
    if client_id.is_empty() || client_secret.is_empty() {
        eprintln!("Error: Set WHOOP_CLIENT_ID and WHOOP_CLIENT_SECRET environment variables.");
        std::process::exit(1);
    }

    // Read existing fit.log
    let (existing_lines, last_date) = read_existing(&args.fit_log);
    let existing = existing_fingerprints(&existing_lines);

    // Determine date range
    let today = Local::now().date_naive();
    let start = args.since.unwrap_or_else(|| {
        last_date
            .map(|d| d + Duration::days(1))
            .unwrap_or(today - Duration::days(30))
    });
    let end = args.until.unwrap_or(today);

    if start > end {
        println!("Nothing to sync: start date ({start}) is after end date ({end}).");
        return;
    }

    println!("Syncing from {start} to {end}...");

    // Authenticate
    let access_token = match authenticate(&client_id, &client_secret, args.reauth).await {
        Ok(token) => token,
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    };

    println!("Authenticated with Whoop.");

    let api = client::WhoopClient::new(access_token);
    let mut new_lines: Vec<(NaiveDate, String)> = Vec::new();

    // Fetch all data concurrently
    let (workouts, sleep, recovery) = tokio::join!(
        api.fetch_workouts(start, end),
        api.fetch_sleep(start, end),
        api.fetch_recovery(start, end),
    );

    // Process workouts
    match workouts {
        Ok(workouts) => {
            for workout in &workouts {
                if let Some(line) = mapping::workout_to_line(workout) {
                    if !existing.contains(&line)
                        && let Some(date) = mapping::parse_iso_date(&workout.end)
                    {
                        new_lines.push((date, line));
                    }
                } else {
                    let name = workout.sport_name.as_deref().unwrap_or("unknown");
                    eprintln!("Skipping unrecognized sport: {name}");
                }
            }
        }
        Err(e) => eprintln!("Warning: Failed to fetch workouts: {e}"),
    }

    // Process sleep
    match sleep {
        Ok(sleeps) => {
            for s in &sleeps {
                // Skip naps
                if s.nap == Some(true) {
                    continue;
                }
                // Only process scored sleep
                if s.score_state.as_deref() != Some("SCORED") {
                    continue;
                }
                if let Some(score) = &s.score {
                    // Compute total sleep time from stage summary
                    let total_sleep_milli = score
                        .stage_summary
                        .as_ref()
                        .map(|ss| {
                            let in_bed = ss.total_in_bed_time_milli.unwrap_or(0);
                            let awake = ss.total_awake_time_milli.unwrap_or(0);
                            in_bed.saturating_sub(awake)
                        })
                        .unwrap_or(0);

                    if total_sleep_milli == 0 {
                        continue;
                    }

                    let perf = score.sleep_performance_percentage.unwrap_or(0.0) as u8;
                    // Use end timestamp as sleep date (the day you wake up)
                    if let Some(date) = mapping::parse_iso_date(&s.end) {
                        let line = mapping::sleep_to_line(date, total_sleep_milli, perf);
                        if !existing.contains(&line) {
                            new_lines.push((date, line));
                        }
                    }
                }
            }
        }
        Err(e) => eprintln!("Warning: Failed to fetch sleep data: {e}"),
    }

    // Process recovery
    match recovery {
        Ok(recoveries) => {
            for r in &recoveries {
                if r.score_state.as_deref() != Some("SCORED") {
                    continue;
                }
                if let Some(score) = &r.score {
                    let pct = score.recovery_score.unwrap_or(0.0) as u8;
                    let hrv = score.hrv_rmssd_milli.unwrap_or(0.0) as u16;
                    let rhr = score.resting_heart_rate.unwrap_or(0.0) as u8;

                    if pct == 0 && hrv == 0 && rhr == 0 {
                        continue;
                    }

                    // Use created_at for the date
                    let date_str = r.created_at.as_deref().unwrap_or("");
                    if let Some(date) = mapping::parse_iso_date(date_str) {
                        let line = mapping::recovery_to_line(date, pct, hrv, rhr);
                        if !existing.contains(&line) {
                            new_lines.push((date, line));
                        }
                    }
                }
            }
        }
        Err(e) => eprintln!("Warning: Failed to fetch recovery data: {e}"),
    }

    // Sort by date
    new_lines.sort_by_key(|(date, _)| *date);

    if new_lines.is_empty() {
        println!("No new records to sync.");
        return;
    }

    println!("Found {} new records:", new_lines.len());
    for (_, line) in &new_lines {
        println!("  {line}");
    }

    if args.dry_run {
        println!("(dry run — nothing written)");
        return;
    }

    // Append to fit.log
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&args.fit_log)
        .unwrap_or_else(|e| {
            eprintln!("Error: Cannot open {}: {e}", args.fit_log);
            std::process::exit(1);
        });

    for (_, line) in &new_lines {
        writeln!(file, "{line}").unwrap_or_else(|e| {
            eprintln!("Error writing to fit.log: {e}");
            std::process::exit(1);
        });
    }

    println!("Wrote {} records to {}", new_lines.len(), args.fit_log);
}

async fn authenticate(
    client_id: &str,
    client_secret: &str,
    force_reauth: bool,
) -> Result<String, String> {
    if !force_reauth && let Some(store) = auth::TokenStore::load() {
        if !store.is_expired() {
            return Ok(store.access_token);
        }
        // Try refresh
        println!("Access token expired, refreshing...");
        match auth::refresh_token(client_id, client_secret, &store.refresh_token).await {
            Ok(new_store) => return Ok(new_store.access_token),
            Err(e) => {
                eprintln!("Token refresh failed ({e}), re-authorizing...");
            }
        }
    }

    let store = auth::authorize(client_id, client_secret).await?;
    Ok(store.access_token)
}
