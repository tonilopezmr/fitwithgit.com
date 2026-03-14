mod client;
mod mapping;

use chrono::{Duration, Local, NaiveDate};
use clap::Parser;
use std::collections::HashSet;
use std::io::Write;

#[derive(Parser)]
#[command(name = "garmin-sync", about = "Sync Garmin Connect data to fit.log")]
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

/// Read existing fit.log lines and find the last date present.
fn read_existing(path: &str) -> (Vec<String>, Option<NaiveDate>) {
    let content = std::fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
    let last_date = lines.iter().filter_map(|l| parse_fit_date(l)).max();
    (lines, last_date)
}

/// Build a set of existing record fingerprints for deduplication.
/// Fingerprint: "CODE,YYMMDD,rest" (the full line).
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

    // Load credentials
    let username = std::env::var("GARMIN_USERNAME").unwrap_or_default();
    let password = std::env::var("GARMIN_PASSWORD").unwrap_or_default();
    if username.is_empty() || password.is_empty() {
        eprintln!("Error: Set GARMIN_USERNAME and GARMIN_PASSWORD environment variables.");
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

    // Login to Garmin Connect
    let sync = match client::GarminSync::login(&username, &password).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    };

    println!("Logged in to Garmin Connect.");

    let mut new_lines: Vec<(NaiveDate, String)> = Vec::new();

    // Fetch daily steps (batch)
    match sync.fetch_daily_steps(start, end).await {
        Ok(daily_steps) => {
            for ds in &daily_steps {
                let line = mapping::steps_to_line(ds.date, ds.steps, ds.goal);
                if !existing.contains(&line) {
                    new_lines.push((ds.date, line));
                }
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
                    if !existing.contains(&line) {
                        new_lines.push((date, line));
                    }
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
