mod data;

use askama::Template;
use askama_web::WebTemplate;
use axum::{Router, extract::Query, routing::get};
use chrono::{Datelike, Duration, NaiveDate};
use serde::Deserialize;
use std::path::Path;
use tower_http::services::ServeDir;

// --- Data model ---

use data::ExerciseDay;

pub struct GraphCell {
    pub date_str: String,
    pub count: u32,
    pub level: u8,
    pub date_label: String,
    pub is_future: bool,
}

pub struct GraphWeek {
    pub cells: Vec<Option<GraphCell>>,
}

pub struct MonthLabel {
    pub name: String,
    pub col_start: usize,
}

#[derive(Deserialize)]
struct ActivityQuery {
    mode: Option<String>,
    activity: Option<String>,
}

pub struct ActivityInfo {
    pub code: String,
    pub emoji: String,
    pub name: String,
    pub active: bool,
}

fn activity_emoji(code: &str) -> &'static str {
    match code {
        "S" => "\u{1F45F}",
        "R" => "\u{1F3C3}",
        "W" => "\u{1F3CA}",
        "B" => "\u{1F6B4}",
        "G" => "\u{1F3CB}\u{FE0F}",
        "X" => "\u{1F9D8}",
        "K" => "\u{26F7}\u{FE0F}",
        "H" => "\u{1F97E}",
        _ => "",
    }
}

fn activity_name(code: &str) -> &'static str {
    match code {
        "S" => "Steps",
        "R" => "Run",
        "W" => "Swim",
        "B" => "Bike",
        "G" => "Gym",
        "X" => "Stretch",
        "K" => "Ski",
        "H" => "Hike",
        _ => "",
    }
}

// --- Graph computation ---

fn compute_level(count: u32, max_count: u32) -> u8 {
    if count == 0 || max_count == 0 {
        return 0;
    }
    let ratio = count as f64 / max_count as f64;
    if ratio <= 0.25 {
        1
    } else if ratio <= 0.50 {
        2
    } else if ratio <= 0.75 {
        3
    } else {
        4
    }
}

fn month_short_name(month: u32) -> &'static str {
    match month {
        1 => "Jan",
        2 => "Feb",
        3 => "Mar",
        4 => "Apr",
        5 => "May",
        6 => "Jun",
        7 => "Jul",
        8 => "Aug",
        9 => "Sep",
        10 => "Oct",
        11 => "Nov",
        12 => "Dec",
        _ => "",
    }
}

fn build_graph(
    data: &[ExerciseDay],
    today: NaiveDate,
    graph_start: NaiveDate,
    graph_end: NaiveDate,
) -> (Vec<GraphWeek>, Vec<MonthLabel>, u32) {
    let max_count = data.iter().map(|d| d.count).max().unwrap_or(0);
    let total_exercises: u32 = data.iter().map(|d| d.count).sum();

    // Build a lookup for exercise data
    let mut day_map: std::collections::HashMap<NaiveDate, u32> = std::collections::HashMap::new();
    for day in data {
        day_map.insert(day.date, day.count);
    }

    let mut weeks: Vec<GraphWeek> = Vec::new();
    let mut current_week: Vec<Option<GraphCell>> = vec![None, None, None, None, None, None, None];
    let mut month_labels: Vec<MonthLabel> = Vec::new();
    let mut last_month: Option<u32> = None;
    let mut week_index: usize = 0;

    let mut current = graph_start;
    while current <= graph_end {
        let weekday_index = current.weekday().num_days_from_sunday() as usize;

        if weekday_index == 0 && current_week.iter().any(|c| c.is_some()) {
            weeks.push(GraphWeek {
                cells: current_week,
            });
            current_week = vec![None, None, None, None, None, None, None];
            week_index += 1;
        }

        let month = current.month();
        if last_month != Some(month) {
            month_labels.push(MonthLabel {
                name: month_short_name(month).to_string(),
                col_start: week_index,
            });
            last_month = Some(month);
        }

        let is_future = current > today;
        let count = if is_future {
            0
        } else {
            day_map.get(&current).copied().unwrap_or(0)
        };
        let level = if is_future {
            0
        } else {
            compute_level(count, max_count)
        };
        let date_label = current.format("%b %d, %Y").to_string();
        let date_str = current.format("%Y-%m-%d").to_string();

        current_week[weekday_index] = Some(GraphCell {
            date_str,
            count,
            level,
            date_label,
            is_future,
        });

        current += Duration::days(1);
    }

    if current_week.iter().any(|c| c.is_some()) {
        weeks.push(GraphWeek {
            cells: current_week,
        });
    }

    (weeks, month_labels, total_exercises)
}

struct BuildResult {
    weeks: Vec<GraphWeek>,
    month_labels: Vec<MonthLabel>,
    header_text: String,
    mode: String,
    activity_filter: String,
    activities: Vec<ActivityInfo>,
}

fn build_activity(mode: &str, activity_filter: Option<&str>) -> BuildResult {
    let today = chrono::Local::now().date_naive();
    let year = today.year();
    let path = Path::new("fit.log");

    let (start_date, graph_end, header_text) = match mode {
        "year" => {
            let jan1 = NaiveDate::from_ymd_opt(year, 1, 1).unwrap();
            let dec31 = NaiveDate::from_ymd_opt(year, 12, 31).unwrap();
            (jan1, dec31, format!("{} exercises in {}", "{total}", year))
        }
        _ => {
            let start = today - Duration::days(364);
            (
                start,
                today,
                "{total} exercises in the last year".to_string(),
            )
        }
    };

    let available = data::get_available_activities(path);
    let filter = activity_filter.filter(|f| available.contains(&f.to_string()));

    let all_days = data::load_exercise_days(path, filter);
    let data: Vec<ExerciseDay> = all_days
        .into_iter()
        .filter(|d| d.date >= start_date && d.date <= today.min(graph_end))
        .collect();
    let (weeks, month_labels, total_exercises) = build_graph(&data, today, start_date, graph_end);

    let header = header_text.replace("{total}", &total_exercises.to_string());
    let activities: Vec<ActivityInfo> = available
        .iter()
        .map(|code| {
            let name = activity_name(code);
            ActivityInfo {
                emoji: activity_emoji(code).to_string(),
                name: if name.is_empty() {
                    code.clone()
                } else {
                    name.to_string()
                },
                active: filter == Some(code.as_str()),
                code: code.clone(),
            }
        })
        .collect();

    BuildResult {
        weeks,
        month_labels,
        header_text: header,
        mode: mode.to_string(),
        activity_filter: filter.unwrap_or("").to_string(),
        activities,
    }
}

// --- Templates ---

#[derive(Template, WebTemplate)]
#[template(path = "index.html")]
struct IndexTemplate {
    weeks: Vec<GraphWeek>,
    month_labels: Vec<MonthLabel>,
    header_text: String,
    mode: String,
    is_htmx: bool,
    activity_filter: String,
    activities: Vec<ActivityInfo>,
}

#[derive(Template, WebTemplate)]
#[template(path = "components/activity_graph.html")]
struct ActivityGraphTemplate {
    weeks: Vec<GraphWeek>,
    month_labels: Vec<MonthLabel>,
    header_text: String,
    mode: String,
    is_htmx: bool,
    activity_filter: String,
    activities: Vec<ActivityInfo>,
}

// --- Handlers ---

async fn index(query: Query<ActivityQuery>) -> IndexTemplate {
    let mode = query.mode.as_deref().unwrap_or("rolling");
    let r = build_activity(mode, query.activity.as_deref());
    IndexTemplate {
        weeks: r.weeks,
        month_labels: r.month_labels,
        header_text: r.header_text,
        mode: r.mode,
        is_htmx: false,
        activity_filter: r.activity_filter,
        activities: r.activities,
    }
}

async fn activity(query: Query<ActivityQuery>) -> ActivityGraphTemplate {
    let mode = query.mode.as_deref().unwrap_or("rolling");
    let r = build_activity(mode, query.activity.as_deref());
    ActivityGraphTemplate {
        weeks: r.weeks,
        month_labels: r.month_labels,
        header_text: r.header_text,
        mode: r.mode,
        is_htmx: true,
        activity_filter: r.activity_filter,
        activities: r.activities,
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let app = Router::new()
        .route("/", get(index))
        .route("/activity", get(activity))
        .nest_service("/static", ServeDir::new("static"));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .expect("Failed to bind to port 3000");

    tracing::info!("Listening on http://localhost:3000");

    axum::serve(listener, app).await.expect("Server failed");
}
