use askama::Template;
use askama_web::WebTemplate;
use axum::{Router, extract::Query, routing::get};
use chrono::{Datelike, Duration, NaiveDate};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde::Deserialize;
use tower_http::services::ServeDir;

// --- Data model ---

struct ExerciseDay {
    date: NaiveDate,
    count: u32,
}

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
}

// --- Mock data generation ---

fn generate_mock_data(start_date: NaiveDate, end_date: NaiveDate) -> Vec<ExerciseDay> {
    let seed = end_date.year() as u64 * 1000 + start_date.ordinal() as u64;
    let mut rng = StdRng::seed_from_u64(seed);
    let mut data = Vec::new();
    let mut current = start_date;
    while current <= end_date {
        let count = if rng.gen_bool(0.3) {
            0
        } else {
            rng.gen_range(1..=8)
        };
        data.push(ExerciseDay {
            date: current,
            count,
        });
        current += Duration::days(1);
    }
    data
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
    graph_end: NaiveDate,
) -> (Vec<GraphWeek>, Vec<MonthLabel>, u32) {
    let max_count = data.iter().map(|d| d.count).max().unwrap_or(0);
    let total_exercises: u32 = data.iter().map(|d| d.count).sum();

    // Build a lookup for exercise data
    let mut day_map: std::collections::HashMap<NaiveDate, u32> = std::collections::HashMap::new();
    for day in data {
        day_map.insert(day.date, day.count);
    }

    // Walk from start of data to graph_end
    let graph_start = if data.is_empty() {
        graph_end
    } else {
        data[0].date
    };

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

fn build_activity(mode: &str) -> (Vec<GraphWeek>, Vec<MonthLabel>, String, String) {
    let today = chrono::Local::now().date_naive();
    let year = today.year();

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

    let data = generate_mock_data(start_date, today.min(graph_end));
    let (weeks, month_labels, total_exercises) = build_graph(&data, today, graph_end);

    let header = header_text.replace("{total}", &total_exercises.to_string());
    (weeks, month_labels, header, mode.to_string())
}

// --- Templates ---

#[derive(Template, WebTemplate)]
#[template(path = "index.html")]
struct IndexTemplate {
    weeks: Vec<GraphWeek>,
    month_labels: Vec<MonthLabel>,
    header_text: String,
    mode: String,
}

#[derive(Template, WebTemplate)]
#[template(path = "components/activity_graph.html")]
struct ActivityGraphTemplate {
    weeks: Vec<GraphWeek>,
    month_labels: Vec<MonthLabel>,
    header_text: String,
    mode: String,
}

// --- Handlers ---

async fn index(query: Query<ActivityQuery>) -> IndexTemplate {
    let mode = query.mode.as_deref().unwrap_or("rolling");
    let (weeks, month_labels, header_text, mode) = build_activity(mode);
    IndexTemplate {
        weeks,
        month_labels,
        header_text,
        mode,
    }
}

async fn activity(query: Query<ActivityQuery>) -> ActivityGraphTemplate {
    let mode = query.mode.as_deref().unwrap_or("rolling");
    let (weeks, month_labels, header_text, mode) = build_activity(mode);
    ActivityGraphTemplate {
        weeks,
        month_labels,
        header_text,
        mode,
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
