mod client;
mod mapping;

use chrono::{Duration, Local, NaiveDate};
use clap::Parser;
use std::collections::{BTreeMap, HashSet};

#[derive(Parser)]
#[command(name = "garmin-sync", about = "Sync Garmin Connect data to fit.log")]
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
}

/// Parse a fit.log line to extract its date (YYMMDD at position 1) and the full line.
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

/// Read existing fit.log lines and find the last date present.
fn read_existing(path: &str) -> (Vec<String>, Option<NaiveDate>) {
    let content = std::fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
    let last_date = lines.iter().filter_map(|l| parse_fit_date(l)).max();
    (lines, last_date)
}

/// Build a set of existing record fingerprints for deduplication.
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

    // Load credentials
    let username = std::env::var("GARMIN_USERNAME").unwrap_or_default();
    let password = std::env::var("GARMIN_PASSWORD").unwrap_or_default();
    if username.is_empty() || password.is_empty() {
        eprintln!("Error: Set GARMIN_USERNAME and GARMIN_PASSWORD environment variables.");
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

    // Login to Garmin Connect
    let sync = match client::GarminSync::login(&username, &password).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    };

    println!("Logged in to Garmin Connect.");

    let mut incoming_lines: Vec<(NaiveDate, String)> = Vec::new();

    // Fetch daily steps (batch)
    match sync.fetch_daily_steps(start, end).await {
        Ok(daily_steps) => {
            for ds in &daily_steps {
                let line = mapping::steps_to_line(ds.date, ds.steps, ds.goal);
                incoming_lines.push((ds.date, line));
            }
        }
        Err(e) => {
            eprintln!("Warning: Failed to fetch daily steps: {e}");
        }
    }

    // Fetch activities
    match sync.fetch_activities(start, end).await {
        Ok(activities) => {
            for activity in &activities {
                let date = activity
                    .start_time_local
                    .as_deref()
                    .and_then(|s| NaiveDate::parse_from_str(&s[..10], "%Y-%m-%d").ok());

                let Some(date) = date else { continue };

                let duration = activity.duration.unwrap_or(0.0);

                // Count laps if the field is a number
                let laps = activity
                    .laps
                    .as_ref()
                    .and_then(|v| v.as_array().map(|a| a.len() as u16));

                let line = mapping::activity_to_line(
                    date,
                    &activity.activity_type.type_key,
                    duration,
                    activity.distance,
                    activity.elevation_gain,
                    laps,
                );

                if let Some(line) = line {
                    incoming_lines.push((date, line));
                } else {
                    eprintln!(
                        "Skipping unknown activity type: {}",
                        activity.activity_type.type_key
                    );
                }
            }
        }
        Err(e) => {
            eprintln!("Warning: Failed to fetch activities: {e}");
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
    fn prepare_sync_replaces_latest_steps_and_keeps_existing_activity() {
        let existing_lines = vec![
            "S,260320,3000,10000".to_string(),
            "R,260320,32,5.1,6.3".to_string(),
        ];
        let incoming_lines = vec![
            (
                NaiveDate::from_ymd_opt(2026, 3, 20).unwrap(),
                "S,260320,5000,10000".to_string(),
            ),
            (
                NaiveDate::from_ymd_opt(2026, 3, 20).unwrap(),
                "R,260320,32,5.1,6.3".to_string(),
            ),
        ];

        let prepared = prepare_sync(&existing_lines, incoming_lines);
        assert_eq!(prepared.new_lines.len(), 1);
        assert_eq!(prepared.new_lines[0].1, "S,260320,5000,10000");

        let merged = merge_lines(&existing_lines, &prepared);
        assert_eq!(
            merged,
            vec![
                "R,260320,32,5.1,6.3".to_string(),
                "S,260320,5000,10000".to_string(),
            ]
        );
    }

    #[test]
    fn merge_lines_keeps_backfills_in_chronological_order() {
        let existing_lines = vec![
            "S,260319,9000,10000".to_string(),
            "S,260320,3000,10000".to_string(),
            "R,260321,32,5.1,6.3".to_string(),
        ];
        let incoming_lines = vec![(
            NaiveDate::from_ymd_opt(2026, 3, 20).unwrap(),
            "S,260320,5000,10000".to_string(),
        )];

        let prepared = prepare_sync(&existing_lines, incoming_lines);
        let merged = merge_lines(&existing_lines, &prepared);

        assert_eq!(
            merged,
            vec![
                "S,260319,9000,10000".to_string(),
                "S,260320,5000,10000".to_string(),
                "R,260321,32,5.1,6.3".to_string(),
            ]
        );
    }
}
