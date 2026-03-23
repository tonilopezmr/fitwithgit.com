mod auth;
mod client;
mod mapping;

use chrono::{Duration, Local, NaiveDate};
use clap::Parser;
use std::collections::{BTreeMap, HashSet};

#[derive(Parser)]
#[command(name = "whoop-sync", about = "Sync Whoop data to fit.log")]
struct Args {
    /// Path to fit.log file
    #[arg(long, default_value = "./fit.log")]
    fit_log: String,

    /// Start date (YYYY-MM-DD). Default: latest fit.log entry date.
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

fn singleton_key(line: &str) -> Option<String> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }

    let parts: Vec<&str> = line.split(',').collect();
    if parts.len() < 2 || parts[1].len() != 6 {
        return None;
    }

    match parts[0] {
        "S" | "G" | "X" | "Z" | "V" => Some(format!("{},{}", parts[0], parts[1])),
        _ => None,
    }
}

fn default_sync_start(last_date: Option<NaiveDate>, today: NaiveDate) -> NaiveDate {
    last_date.unwrap_or(today - Duration::days(30))
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

struct PreparedSync {
    new_lines: Vec<(NaiveDate, String)>,
    replacement_keys: HashSet<String>,
}

fn prepare_sync(
    existing_lines: &[String],
    incoming_lines: Vec<(NaiveDate, String)>,
) -> PreparedSync {
    let existing = existing_fingerprints(existing_lines);
    let mut singleton_updates: BTreeMap<String, (NaiveDate, String)> = BTreeMap::new();
    let mut activity_updates: Vec<(NaiveDate, String)> = Vec::new();
    let mut seen_activity_lines: HashSet<String> = HashSet::new();

    for (date, line) in incoming_lines {
        if let Some(key) = singleton_key(&line) {
            if !existing.contains(&line) {
                singleton_updates.insert(key, (date, line));
            }
        } else if !existing.contains(&line) && seen_activity_lines.insert(line.clone()) {
            activity_updates.push((date, line));
        }
    }

    let replacement_keys = singleton_updates.keys().cloned().collect::<HashSet<_>>();
    let mut new_lines = singleton_updates.into_values().collect::<Vec<_>>();
    new_lines.extend(activity_updates);
    new_lines.sort_by_key(|(date, _)| *date);

    PreparedSync {
        new_lines,
        replacement_keys,
    }
}

fn merge_lines(existing_lines: &[String], prepared: &PreparedSync) -> Vec<String> {
    let mut merged = existing_lines
        .iter()
        .filter(|line| {
            singleton_key(line)
                .as_ref()
                .is_none_or(|key| !prepared.replacement_keys.contains(key))
        })
        .cloned()
        .collect::<Vec<_>>();

    merged.extend(prepared.new_lines.iter().map(|(_, line)| line.clone()));
    merged.sort_by_key(|line| parse_fit_date(line));
    merged
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

    // Determine date range
    let today = Local::now().date_naive();
    let start = args
        .since
        .unwrap_or_else(|| default_sync_start(last_date, today));
    let end = args.until.unwrap_or(today);

    if start > end {
        println!("Already up to date.");
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
    let mut incoming_lines: Vec<(NaiveDate, String)> = Vec::new();

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
                    if let Some(date) = mapping::parse_iso_date(&workout.end) {
                        incoming_lines.push((date, line));
                    }
                } else {
                    let name = workout.sport_name.as_deref().unwrap_or("unknown");
                    eprintln!("Skipping unrecognized sport: {name}");
                }
            }
        }
        Err(e) => {
            eprintln!("Warning: Failed to fetch workouts: {e}");
        }
    }

    // Process sleep
    match sleep {
        Ok(sleeps) => {
            for s in &sleeps {
                if s.nap == Some(true) {
                    continue;
                }
                if s.score_state.as_deref() != Some("SCORED") {
                    continue;
                }
                if let Some(score) = &s.score {
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
                    if let Some(date) = mapping::parse_iso_date(&s.end) {
                        let line = mapping::sleep_to_line(date, total_sleep_milli, perf);
                        incoming_lines.push((date, line));
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("Warning: Failed to fetch sleep data: {e}");
        }
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

                    let date_str = r.created_at.as_deref().unwrap_or("");
                    if let Some(date) = mapping::parse_iso_date(date_str) {
                        let line = mapping::recovery_to_line(date, pct, hrv, rhr);
                        incoming_lines.push((date, line));
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("Warning: Failed to fetch recovery: {e}");
        }
    }

    let prepared = prepare_sync(&existing_lines, incoming_lines);

    if prepared.new_lines.is_empty() {
        println!("No new records to sync.");
        return;
    }

    println!("Found {} new/updated records:", prepared.new_lines.len());
    for (_, line) in &prepared.new_lines {
        println!("  {line}");
    }

    if args.dry_run {
        println!("(dry run — nothing written)");
        return;
    }

    let merged_lines = merge_lines(&existing_lines, &prepared);
    let output = if merged_lines.is_empty() {
        String::new()
    } else {
        let mut content = merged_lines.join("\n");
        content.push('\n');
        content
    };

    std::fs::write(&args.fit_log, output).unwrap_or_else(|e| {
        eprintln!("Error writing to {}: {e}", args.fit_log);
        std::process::exit(1);
    });

    println!(
        "Wrote {} records to {}",
        prepared.new_lines.len(),
        args.fit_log
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_start_rechecks_latest_day() {
        let today = NaiveDate::from_ymd_opt(2026, 3, 20).unwrap();
        let last = NaiveDate::from_ymd_opt(2026, 3, 15).unwrap();
        assert_eq!(default_sync_start(Some(last), today), last);
    }

    #[test]
    fn singleton_keys_cover_daily_summary_codes() {
        assert_eq!(
            singleton_key("S,260320,5000,10000"),
            Some("S,260320".into())
        );
        assert_eq!(singleton_key("G,260320,1"), Some("G,260320".into()));
        assert_eq!(singleton_key("X,260320"), Some("X,260320".into()));
        assert_eq!(singleton_key("Z,260320,462,85"), Some("Z,260320".into()));
        assert_eq!(singleton_key("V,260320,78,65,52"), Some("V,260320".into()));
        assert_eq!(singleton_key("R,260320,32,5.1,6.3"), None);
    }

    #[test]
    fn prepare_sync_replaces_latest_sleep_and_keeps_existing_activity() {
        let existing_lines = vec![
            "Z,260320,400,80".to_string(),
            "R,260320,32,5.1,6.3".to_string(),
        ];
        let incoming_lines = vec![
            (
                NaiveDate::from_ymd_opt(2026, 3, 20).unwrap(),
                "Z,260320,462,85".to_string(),
            ),
            (
                NaiveDate::from_ymd_opt(2026, 3, 20).unwrap(),
                "R,260320,32,5.1,6.3".to_string(),
            ),
        ];

        let prepared = prepare_sync(&existing_lines, incoming_lines);
        assert_eq!(prepared.new_lines.len(), 1);
        assert_eq!(prepared.new_lines[0].1, "Z,260320,462,85");

        let merged = merge_lines(&existing_lines, &prepared);
        assert_eq!(
            merged,
            vec![
                "R,260320,32,5.1,6.3".to_string(),
                "Z,260320,462,85".to_string(),
            ]
        );
    }

    #[test]
    fn merge_lines_keeps_backfills_in_chronological_order() {
        let existing_lines = vec![
            "Z,260319,430,82".to_string(),
            "Z,260320,400,80".to_string(),
            "R,260321,32,5.1,6.3".to_string(),
        ];
        let incoming_lines = vec![(
            NaiveDate::from_ymd_opt(2026, 3, 20).unwrap(),
            "Z,260320,462,85".to_string(),
        )];

        let prepared = prepare_sync(&existing_lines, incoming_lines);
        let merged = merge_lines(&existing_lines, &prepared);

        assert_eq!(
            merged,
            vec![
                "Z,260319,430,82".to_string(),
                "Z,260320,462,85".to_string(),
                "R,260321,32,5.1,6.3".to_string(),
            ]
        );
    }
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
