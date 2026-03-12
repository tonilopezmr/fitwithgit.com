use askama::Template;
use askama_web::WebTemplate;
use axum::{Router, routing::get};
use chrono::{Datelike, Duration, NaiveDate};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
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
    pub is_today: bool,
}

pub struct GraphWeek {
    pub cells: Vec<Option<GraphCell>>,
}

pub struct MonthLabel {
    pub name: String,
    pub col_start: usize,
}

// --- Mock data generation ---

fn generate_mock_data(end_date: NaiveDate) -> Vec<ExerciseDay> {
    let seed = end_date.year() as u64 * 1000 + end_date.ordinal() as u64;
    let mut rng = StdRng::seed_from_u64(seed);
    let start_date = end_date - Duration::days(364);
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

fn build_graph(data: &[ExerciseDay], today: NaiveDate) -> (Vec<GraphWeek>, Vec<MonthLabel>, u32) {
    let max_count = data.iter().map(|d| d.count).max().unwrap_or(0);
    let total_exercises: u32 = data.iter().map(|d| d.count).sum();

    let mut weeks: Vec<GraphWeek> = Vec::new();
    let mut current_week: Vec<Option<GraphCell>> = vec![None, None, None, None, None, None, None];
    let mut month_labels: Vec<MonthLabel> = Vec::new();
    let mut last_month: Option<u32> = None;
    let mut week_index: usize = 0;

    for day in data {
        let weekday_index = day.date.weekday().num_days_from_sunday() as usize;

        // Start a new week when we hit Sunday and the current week has data
        if weekday_index == 0 && current_week.iter().any(|c| c.is_some()) {
            weeks.push(GraphWeek {
                cells: current_week,
            });
            current_week = vec![None, None, None, None, None, None, None];
            week_index += 1;
        }

        // Track month transitions for labels
        let month = day.date.month();
        if last_month != Some(month) {
            month_labels.push(MonthLabel {
                name: month_short_name(month).to_string(),
                col_start: week_index,
            });
            last_month = Some(month);
        }

        let level = compute_level(day.count, max_count);
        let day_num = day.date.day();
        let suffix = match (day_num % 10, day_num % 100) {
            (1, 11) | (2, 12) | (3, 13) => "th",
            (1, _) => "st",
            (2, _) => "nd",
            (3, _) => "rd",
            _ => "th",
        };
        let date_label = format!("{} {}{}", day.date.format("%B"), day_num, suffix);
        let date_str = day.date.format("%Y-%m-%d").to_string();

        current_week[weekday_index] = Some(GraphCell {
            date_str,
            count: day.count,
            level,
            date_label,
            is_today: day.date == today,
        });
    }

    // Push final partial week
    if current_week.iter().any(|c| c.is_some()) {
        weeks.push(GraphWeek {
            cells: current_week,
        });
    }

    (weeks, month_labels, total_exercises)
}

// --- Templates ---

#[derive(Template, WebTemplate)]
#[template(path = "index.html")]
struct IndexTemplate {
    weeks: Vec<GraphWeek>,
    month_labels: Vec<MonthLabel>,
    total_exercises: u32,
}

#[derive(Template, WebTemplate)]
#[template(path = "components/activity_graph.html")]
struct ActivityGraphTemplate {
    weeks: Vec<GraphWeek>,
    month_labels: Vec<MonthLabel>,
    total_exercises: u32,
}

// --- Handlers ---

async fn index() -> IndexTemplate {
    let today = chrono::Local::now().date_naive();
    let data = generate_mock_data(today);
    let (weeks, month_labels, total_exercises) = build_graph(&data, today);
    IndexTemplate {
        weeks,
        month_labels,
        total_exercises,
    }
}

async fn activity() -> ActivityGraphTemplate {
    let today = chrono::Local::now().date_naive();
    let data = generate_mock_data(today);
    let (weeks, month_labels, total_exercises) = build_graph(&data, today);
    ActivityGraphTemplate {
        weeks,
        month_labels,
        total_exercises,
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
