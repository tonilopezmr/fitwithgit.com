use chrono::{Datelike, NaiveDate};

use crate::client::WhoopWorkout;

/// Maps a Whoop sport name to a fit.log activity code.
/// Uses sport_name (string) as primary, with sport_id (integer) as fallback.
/// Returns None for unrecognized sports (they will be skipped).
pub fn map_whoop_sport(sport_name: Option<&str>, sport_id: Option<u32>) -> Option<&'static str> {
    // Try sport_name first (preferred, as sport_id is deprecated)
    if let Some(name) = sport_name {
        let code = match name.to_lowercase().as_str() {
            "running" | "track & field" | "stroller jogging" => Some("R"),
            "cycling" | "mountain biking" | "spin" | "assault bike" => Some("B"),
            "swimming" | "water polo" => Some("W"),
            "hiking/rucking" | "walking" | "stroller walking" | "dog walking" => Some("H"),
            "weightlifting"
            | "powerlifting"
            | "functional fitness"
            | "strength trainer"
            | "hiit"
            | "obstacle course racing"
            | "box fitness"
            | "f45 training"
            | "barry's" => Some("G"),
            "yoga" | "hot yoga" | "pilates" | "stretching" | "barre" | "barre3" | "meditation"
            | "gymnastics" => Some("X"),
            "skiing" | "cross country skiing" | "snowboarding" => Some("K"),
            _ => None,
        };
        if code.is_some() {
            return code;
        }
    }

    // Fallback to sport_id
    if let Some(id) = sport_id {
        return match id {
            0 | 35 | 253 => Some("R"),      // Running, Track & Field, Stroller Jogging
            1 | 57 | 97 | 126 => Some("B"), // Cycling, Mountain Biking, Spin, Assault Bike
            33 | 37 => Some("W"),           // Swimming, Water Polo
            52 | 63 | 252 | 266 => Some("H"), // Hiking/Rucking, Walking, Stroller Walking, Dog Walking
            45 | 59 | 48 | 123 | 96 | 94 | 103 | 248 | 250 => Some("G"), // Weightlifting, Powerlifting, etc.
            44 | 259 | 43 | 128 | 107 | 258 | 70 | 51 => Some("X"), // Yoga, Pilates, Stretching, etc.
            29 | 47 | 91 => Some("K"), // Skiing, XC Skiing, Snowboarding
            _ => None,
        };
    }

    None
}

fn format_date(date: NaiveDate) -> String {
    format!(
        "{:02}{:02}{:02}",
        date.year_ce().1 % 100,
        date.month(),
        date.day()
    )
}

/// Convert a Whoop workout to a fit.log line.
/// Returns None if the sport type is unrecognized.
pub fn workout_to_line(workout: &WhoopWorkout) -> Option<String> {
    let code = map_whoop_sport(workout.sport_name.as_deref(), workout.sport_id)?;

    // Use end timestamp for date (workout day)
    let date = parse_iso_date(&workout.end)?;
    let d = format_date(date);

    // Compute duration from start/end timestamps
    let duration_min = compute_duration_min(&workout.start, &workout.end);

    let score = workout.score.as_ref();
    let distance_m = score.and_then(|s| s.distance_meter).unwrap_or(0.0);
    let elevation = score.and_then(|s| s.altitude_gain_meter).unwrap_or(0.0);

    let line = match code {
        "R" => {
            let distance_km = distance_m / 1000.0;
            let pace = if distance_km > 0.0 {
                duration_min as f32 / distance_km as f32
            } else {
                0.0
            };
            format!("R,{d},{duration_min},{distance_km:.1},{pace:.1}")
        }
        "B" => {
            let distance_km = distance_m / 1000.0;
            let avg_speed = if duration_min > 0 {
                distance_km / (duration_min as f64 / 60.0)
            } else {
                0.0
            };
            format!("B,{d},{duration_min},{distance_km:.1},{avg_speed:.1}")
        }
        "W" => {
            let distance = distance_m as u32;
            // Whoop doesn't track laps
            format!("W,{d},{duration_min},{distance},0")
        }
        "H" => {
            let distance_km = distance_m / 1000.0;
            let elev = elevation as u32;
            format!("H,{d},{duration_min},{distance_km:.1},{elev}")
        }
        "G" => format!("G,{d},1"),
        "X" => format!("X,{d}"),
        "K" => {
            // Whoop doesn't track ski runs
            format!("K,{d},{duration_min},0")
        }
        _ => return None,
    };
    Some(line)
}

/// Convert Whoop sleep data to a fit.log line.
/// Uses the end timestamp as the date (the day you wake up).
pub fn sleep_to_line(date: NaiveDate, total_sleep_milli: u64, score: u8) -> String {
    let duration_min = (total_sleep_milli / 60_000) as u16;
    format!("Z,{},{duration_min},{score}", format_date(date))
}

/// Convert Whoop recovery data to a fit.log line.
pub fn recovery_to_line(date: NaiveDate, recovery_pct: u8, hrv: u16, rhr: u8) -> String {
    format!("V,{},{recovery_pct},{hrv},{rhr}", format_date(date))
}

/// Parse an ISO 8601 timestamp to extract the date portion.
pub fn parse_iso_date(iso: &str) -> Option<NaiveDate> {
    // Handle "2024-01-15T08:30:00.000Z" or "2024-01-15"
    let date_str = if iso.len() >= 10 { &iso[..10] } else { iso };
    NaiveDate::parse_from_str(date_str, "%Y-%m-%d").ok()
}

fn compute_duration_min(start: &str, end: &str) -> u16 {
    let parse = |s: &str| -> Option<chrono::NaiveDateTime> {
        // Try with milliseconds first, then without
        chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.fZ")
            .or_else(|_| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%SZ"))
            .ok()
    };

    match (parse(start), parse(end)) {
        (Some(s), Some(e)) => {
            let diff = e.signed_duration_since(s);
            (diff.num_seconds().max(0) / 60) as u16
        }
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_whoop_sport_by_name() {
        assert_eq!(map_whoop_sport(Some("Running"), None), Some("R"));
        assert_eq!(map_whoop_sport(Some("Cycling"), None), Some("B"));
        assert_eq!(map_whoop_sport(Some("Swimming"), None), Some("W"));
        assert_eq!(map_whoop_sport(Some("Hiking/Rucking"), None), Some("H"));
        assert_eq!(map_whoop_sport(Some("Weightlifting"), None), Some("G"));
        assert_eq!(map_whoop_sport(Some("Yoga"), None), Some("X"));
        assert_eq!(map_whoop_sport(Some("Skiing"), None), Some("K"));
        assert_eq!(map_whoop_sport(Some("Snowboarding"), None), Some("K"));
        assert_eq!(map_whoop_sport(Some("Unknown Sport"), None), None);
    }

    #[test]
    fn test_map_whoop_sport_by_id_fallback() {
        assert_eq!(map_whoop_sport(None, Some(0)), Some("R"));
        assert_eq!(map_whoop_sport(None, Some(1)), Some("B"));
        assert_eq!(map_whoop_sport(None, Some(33)), Some("W"));
        assert_eq!(map_whoop_sport(None, Some(52)), Some("H"));
        assert_eq!(map_whoop_sport(None, Some(45)), Some("G"));
        assert_eq!(map_whoop_sport(None, Some(44)), Some("X"));
        assert_eq!(map_whoop_sport(None, Some(29)), Some("K"));
        assert_eq!(map_whoop_sport(None, Some(999)), None);
    }

    #[test]
    fn test_map_whoop_sport_name_takes_priority() {
        // Even with a mismatched sport_id, sport_name should win
        assert_eq!(map_whoop_sport(Some("Running"), Some(1)), Some("R"));
    }

    #[test]
    fn test_sleep_to_line() {
        let date = NaiveDate::from_ymd_opt(2026, 3, 12).unwrap();
        assert_eq!(sleep_to_line(date, 27_720_000, 85), "Z,260312,462,85");
    }

    #[test]
    fn test_recovery_to_line() {
        let date = NaiveDate::from_ymd_opt(2026, 3, 12).unwrap();
        assert_eq!(recovery_to_line(date, 78, 65, 52), "V,260312,78,65,52");
    }

    #[test]
    fn test_parse_iso_date() {
        let date = parse_iso_date("2026-03-12T08:30:00.000Z").unwrap();
        assert_eq!(date, NaiveDate::from_ymd_opt(2026, 3, 12).unwrap());
    }

    #[test]
    fn test_compute_duration_min() {
        assert_eq!(
            compute_duration_min("2026-03-12T06:00:00.000Z", "2026-03-12T06:32:00.000Z"),
            32
        );
    }

    #[test]
    fn test_workout_to_line_run() {
        let workout = WhoopWorkout {
            id: serde_json::Value::String("test".into()),
            sport_name: Some("Running".into()),
            sport_id: Some(0),
            start: "2026-03-12T06:00:00.000Z".into(),
            end: "2026-03-12T06:32:00.000Z".into(),
            score_state: Some("SCORED".into()),
            score: Some(crate::client::WorkoutScore {
                strain: None,
                average_heart_rate: None,
                max_heart_rate: None,
                kilojoule: None,
                distance_meter: Some(5100.0),
                altitude_gain_meter: None,
                zone_duration: None,
            }),
        };
        let line = workout_to_line(&workout).unwrap();
        assert_eq!(line, "R,260312,32,5.1,6.3");
    }

    #[test]
    fn test_workout_to_line_gym() {
        let workout = WhoopWorkout {
            id: serde_json::Value::String("test".into()),
            sport_name: Some("Weightlifting".into()),
            sport_id: Some(45),
            start: "2026-03-12T10:00:00.000Z".into(),
            end: "2026-03-12T11:00:00.000Z".into(),
            score_state: Some("SCORED".into()),
            score: None,
        };
        let line = workout_to_line(&workout).unwrap();
        assert_eq!(line, "G,260312,1");
    }

    #[test]
    fn test_workout_to_line_unknown() {
        let workout = WhoopWorkout {
            id: serde_json::Value::String("test".into()),
            sport_name: Some("Cricket".into()),
            sport_id: Some(100),
            start: "2026-03-12T10:00:00.000Z".into(),
            end: "2026-03-12T11:00:00.000Z".into(),
            score_state: None,
            score: None,
        };
        assert!(workout_to_line(&workout).is_none());
    }
}
