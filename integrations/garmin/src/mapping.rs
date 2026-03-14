use chrono::NaiveDate;

/// Maps a Garmin Connect activity typeKey to a fit.log activity code.
/// Returns None for unrecognized types (they will be skipped).
pub fn map_garmin_type(type_key: &str) -> Option<&'static str> {
    match type_key {
        "running" | "trail_running" | "treadmill_running" => Some("R"),
        "cycling" | "mountain_biking" | "indoor_cycling" | "road_biking" | "gravel_cycling" => {
            Some("B")
        }
        "lap_swimming" | "open_water_swimming" => Some("W"),
        "hiking" | "walking" => Some("H"),
        "strength_training" => Some("G"),
        "yoga" | "pilates" | "breathwork" | "flexibility" => Some("X"),
        "resort_skiing" | "backcountry_skiing" | "snowboarding" | "cross_country_skiing" => {
            Some("K")
        }
        _ => None,
    }
}

fn format_date(date: NaiveDate) -> String {
    format!(
        "{:02}{:02}{:02}",
        date.year_ce().1 % 100,
        date.month(),
        date.day()
    )
}

use chrono::Datelike;

/// Convert a steps daily summary to a fit.log line.
pub fn steps_to_line(date: NaiveDate, steps: u32, goal: u32) -> String {
    format!("S,{},{},{}", format_date(date), steps, goal)
}

/// Convert a Garmin activity to a fit.log line.
/// Returns None if the activity type is unrecognized.
pub fn activity_to_line(
    date: NaiveDate,
    type_key: &str,
    duration_secs: f64,
    distance_meters: Option<f64>,
    elevation_gain: Option<f64>,
    laps: Option<u16>,
) -> Option<String> {
    let code = map_garmin_type(type_key)?;
    let d = format_date(date);
    let duration_min = (duration_secs / 60.0) as u16;

    let line = match code {
        "R" => {
            let distance_km = distance_meters.unwrap_or(0.0) / 1000.0;
            let pace = if distance_km > 0.0 {
                duration_min as f32 / distance_km as f32
            } else {
                0.0
            };
            format!("R,{d},{duration_min},{distance_km:.1},{pace:.1}")
        }
        "B" => {
            let distance_km = distance_meters.unwrap_or(0.0) / 1000.0;
            let avg_speed = if duration_min > 0 {
                distance_km / (duration_min as f64 / 60.0)
            } else {
                0.0
            };
            format!("B,{d},{duration_min},{distance_km:.1},{avg_speed:.1}")
        }
        "W" => {
            let distance_m = distance_meters.unwrap_or(0.0) as u32;
            let lap_count = laps.unwrap_or(0);
            format!("W,{d},{duration_min},{distance_m},{lap_count}")
        }
        "H" => {
            let distance_km = distance_meters.unwrap_or(0.0) / 1000.0;
            let elev = elevation_gain.unwrap_or(0.0) as u32;
            format!("H,{d},{duration_min},{distance_km:.1},{elev}")
        }
        "G" => format!("G,{d},1"),
        "X" => format!("X,{d}"),
        "K" => {
            // Garmin doesn't provide ski run count directly
            format!("K,{d},{duration_min},0")
        }
        _ => return None,
    };
    Some(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_garmin_type() {
        assert_eq!(map_garmin_type("running"), Some("R"));
        assert_eq!(map_garmin_type("trail_running"), Some("R"));
        assert_eq!(map_garmin_type("cycling"), Some("B"));
        assert_eq!(map_garmin_type("lap_swimming"), Some("W"));
        assert_eq!(map_garmin_type("hiking"), Some("H"));
        assert_eq!(map_garmin_type("strength_training"), Some("G"));
        assert_eq!(map_garmin_type("yoga"), Some("X"));
        assert_eq!(map_garmin_type("resort_skiing"), Some("K"));
        assert_eq!(map_garmin_type("unknown_type"), None);
    }

    #[test]
    fn test_steps_to_line() {
        let date = NaiveDate::from_ymd_opt(2026, 3, 12).unwrap();
        assert_eq!(steps_to_line(date, 8500, 10000), "S,260312,8500,10000");
    }

    #[test]
    fn test_activity_to_line_run() {
        let date = NaiveDate::from_ymd_opt(2026, 3, 12).unwrap();
        let line = activity_to_line(date, "running", 1920.0, Some(5100.0), None, None).unwrap();
        assert_eq!(line, "R,260312,32,5.1,6.3");
    }

    #[test]
    fn test_activity_to_line_gym() {
        let date = NaiveDate::from_ymd_opt(2026, 3, 12).unwrap();
        let line = activity_to_line(date, "strength_training", 3600.0, None, None, None).unwrap();
        assert_eq!(line, "G,260312,1");
    }

    #[test]
    fn test_activity_to_line_unknown() {
        let date = NaiveDate::from_ymd_opt(2026, 3, 12).unwrap();
        assert!(activity_to_line(date, "golf", 3600.0, None, None, None).is_none());
    }
}
