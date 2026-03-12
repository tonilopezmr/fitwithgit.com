use chrono::{Datelike, NaiveDate};
use std::collections::BTreeMap;
use std::path::Path;

// --- Activity data model ---

#[allow(dead_code)]
pub enum Activity {
    Steps {
        steps: u32,
        goal: u32,
    },
    Run {
        duration: u16,
        distance_km: f32,
        pace: f32,
    },
    Swim {
        duration: u16,
        distance_m: u32,
        laps: u16,
    },
    Bike {
        duration: u16,
        distance_km: f32,
        avg_speed: f32,
    },
    Gym {
        sessions: u8,
    },
    Stretch,
    Ski {
        duration: u16,
        runs: u8,
    },
    Hike {
        duration: u16,
        distance_km: f32,
        elevation_m: u32,
    },
}

#[allow(dead_code)]
pub struct ActivityRecord {
    pub date: NaiveDate,
    pub activity: Activity,
}

pub struct ExerciseDay {
    pub date: NaiveDate,
    pub count: u32,
}

// --- Parsing ---

fn parse_date(s: &str) -> Option<NaiveDate> {
    if s.len() != 6 {
        return None;
    }
    let y = 2000 + s[0..2].parse::<i32>().ok()?;
    let m = s[2..4].parse::<u32>().ok()?;
    let d = s[4..6].parse::<u32>().ok()?;
    NaiveDate::from_ymd_opt(y, m, d)
}

fn parse_line(line: &str) -> Option<ActivityRecord> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let parts: Vec<&str> = line.split(',').collect();
    if parts.len() < 2 {
        return None;
    }
    let date = parse_date(parts[1])?;
    let activity = match parts[0] {
        "S" if parts.len() >= 4 => Activity::Steps {
            steps: parts[2].parse().ok()?,
            goal: parts[3].parse().ok()?,
        },
        "R" if parts.len() >= 5 => Activity::Run {
            duration: parts[2].parse().ok()?,
            distance_km: parts[3].parse().ok()?,
            pace: parts[4].parse().ok()?,
        },
        "W" if parts.len() >= 5 => Activity::Swim {
            duration: parts[2].parse().ok()?,
            distance_m: parts[3].parse().ok()?,
            laps: parts[4].parse().ok()?,
        },
        "B" if parts.len() >= 5 => Activity::Bike {
            duration: parts[2].parse().ok()?,
            distance_km: parts[3].parse().ok()?,
            avg_speed: parts[4].parse().ok()?,
        },
        "G" if parts.len() >= 3 => Activity::Gym {
            sessions: parts[2].parse().ok()?,
        },
        "X" => Activity::Stretch,
        "K" if parts.len() >= 4 => Activity::Ski {
            duration: parts[2].parse().ok()?,
            runs: parts[3].parse().ok()?,
        },
        "H" if parts.len() >= 5 => Activity::Hike {
            duration: parts[2].parse().ok()?,
            distance_km: parts[3].parse().ok()?,
            elevation_m: parts[4].parse().ok()?,
        },
        _ => return None,
    };
    Some(ActivityRecord { date, activity })
}

pub fn parse_file(path: &Path) -> Vec<ActivityRecord> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    content.lines().filter_map(parse_line).collect()
}

pub fn load_exercise_days(path: &Path) -> Vec<ExerciseDay> {
    let records = parse_file(path);
    let mut counts: BTreeMap<NaiveDate, u32> = BTreeMap::new();
    for r in records {
        *counts.entry(r.date).or_insert(0) += 1;
    }
    counts
        .into_iter()
        .map(|(date, count)| ExerciseDay { date, count })
        .collect()
}

// --- Serialization ---

#[allow(dead_code)]
pub fn format_date(date: NaiveDate) -> String {
    format!(
        "{:02}{:02}{:02}",
        date.year() % 100,
        date.month(),
        date.day()
    )
}

#[allow(dead_code)]
pub fn format_record(record: &ActivityRecord) -> String {
    let d = format_date(record.date);
    match &record.activity {
        Activity::Steps { steps, goal } => format!("S,{d},{steps},{goal}"),
        Activity::Run {
            duration,
            distance_km,
            pace,
        } => format!("R,{d},{duration},{distance_km:.1},{pace:.1}"),
        Activity::Swim {
            duration,
            distance_m,
            laps,
        } => format!("W,{d},{duration},{distance_m},{laps}"),
        Activity::Bike {
            duration,
            distance_km,
            avg_speed,
        } => format!("B,{d},{duration},{distance_km:.1},{avg_speed:.1}"),
        Activity::Gym { sessions } => format!("G,{d},{sessions}"),
        Activity::Stretch => format!("X,{d}"),
        Activity::Ski { duration, runs } => format!("K,{d},{duration},{runs}"),
        Activity::Hike {
            duration,
            distance_km,
            elevation_m,
        } => format!("H,{d},{duration},{distance_km:.1},{elevation_m}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_date() {
        let d = parse_date("260312").unwrap();
        assert_eq!(d, NaiveDate::from_ymd_opt(2026, 3, 12).unwrap());
    }

    #[test]
    fn test_parse_date_invalid() {
        assert!(parse_date("abc").is_none());
        assert!(parse_date("261301").is_none()); // month 13
    }

    #[test]
    fn test_parse_steps() {
        let r = parse_line("S,260312,8500,10000").unwrap();
        assert_eq!(r.date, NaiveDate::from_ymd_opt(2026, 3, 12).unwrap());
        match r.activity {
            Activity::Steps { steps, goal } => {
                assert_eq!(steps, 8500);
                assert_eq!(goal, 10000);
            }
            _ => panic!("expected Steps"),
        }
    }

    #[test]
    fn test_parse_run() {
        let r = parse_line("R,260302,32,5.1,6.3").unwrap();
        match r.activity {
            Activity::Run {
                duration,
                distance_km,
                pace,
            } => {
                assert_eq!(duration, 32);
                assert!((distance_km - 5.1).abs() < 0.01);
                assert!((pace - 6.3).abs() < 0.01);
            }
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn test_parse_stretch() {
        let r = parse_line("X,260301").unwrap();
        assert!(matches!(r.activity, Activity::Stretch));
    }

    #[test]
    fn test_parse_gym() {
        let r = parse_line("G,260301,2").unwrap();
        match r.activity {
            Activity::Gym { sessions } => assert_eq!(sessions, 2),
            _ => panic!("expected Gym"),
        }
    }

    #[test]
    fn test_parse_comment_and_empty() {
        assert!(parse_line("# comment").is_none());
        assert!(parse_line("").is_none());
        assert!(parse_line("  ").is_none());
    }

    #[test]
    fn test_parse_unknown_code() {
        assert!(parse_line("Z,260312,1,2,3").is_none());
    }

    #[test]
    fn test_roundtrip_steps() {
        let record = ActivityRecord {
            date: NaiveDate::from_ymd_opt(2026, 3, 12).unwrap(),
            activity: Activity::Steps {
                steps: 8500,
                goal: 10000,
            },
        };
        let line = format_record(&record);
        assert_eq!(line, "S,260312,8500,10000");
        let parsed = parse_line(&line).unwrap();
        match parsed.activity {
            Activity::Steps { steps, goal } => {
                assert_eq!(steps, 8500);
                assert_eq!(goal, 10000);
            }
            _ => panic!("expected Steps"),
        }
    }

    #[test]
    fn test_roundtrip_stretch() {
        let record = ActivityRecord {
            date: NaiveDate::from_ymd_opt(2026, 1, 5).unwrap(),
            activity: Activity::Stretch,
        };
        let line = format_record(&record);
        assert_eq!(line, "X,260105");
        let parsed = parse_line(&line).unwrap();
        assert!(matches!(parsed.activity, Activity::Stretch));
    }

    #[test]
    fn test_format_date() {
        let d = NaiveDate::from_ymd_opt(2026, 3, 5).unwrap();
        assert_eq!(format_date(d), "260305");
    }

    #[test]
    fn test_load_exercise_days_counts() {
        use std::io::Write;
        let dir = std::env::temp_dir().join("fit_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.log");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "S,260312,8500,10000").unwrap();
        writeln!(f, "G,260312,1").unwrap();
        writeln!(f, "X,260312").unwrap();
        writeln!(f, "S,260311,7000,10000").unwrap();
        drop(f);

        let days = load_exercise_days(&path);
        assert_eq!(days.len(), 2);
        assert_eq!(days[0].count, 1); // 260311: 1 activity
        assert_eq!(days[1].count, 3); // 260312: 3 activities

        std::fs::remove_file(&path).ok();
    }
}
