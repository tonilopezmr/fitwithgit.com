use askama::Template;
use askama_web::WebTemplate;
use axum::{Router, routing::get};
use axum_htmx::HxRequest;
use tower_http::services::ServeDir;

#[derive(Template, WebTemplate)]
#[template(path = "index.html")]
struct IndexTemplate {
    title: String,
}

#[derive(Template, WebTemplate)]
#[template(path = "components/greeting.html")]
struct GreetingTemplate {
    message: String,
}

async fn index() -> IndexTemplate {
    IndexTemplate {
        title: "Tirana - Fit with Git".to_string(),
    }
}

async fn greeting(HxRequest(is_htmx): HxRequest) -> GreetingTemplate {
    let message = if is_htmx {
        "Hello from the server via htmx!".to_string()
    } else {
        "Hello, World! (direct request)".to_string()
    };
    GreetingTemplate { message }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let app = Router::new()
        .route("/", get(index))
        .route("/greeting", get(greeting))
        .nest_service("/static", ServeDir::new("static"));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .expect("Failed to bind to port 3000");

    tracing::info!("Listening on http://localhost:3000");

    axum::serve(listener, app).await.expect("Server failed");
}
