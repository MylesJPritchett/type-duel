pub mod dictionary;
pub mod game;
pub mod handlers;

use axum::Router;
use axum::routing::{get, post};
use game::AppState;

/// Build the application router
pub fn app() -> Router {
    let state = AppState::new();
    app_with_state(state)
}

/// Build the router with a given state
pub fn app_with_state(state: AppState) -> Router {
    Router::new()
        .route("/", get(handlers::index))
        .route("/create-room", post(handlers::create_room))
        .route("/game/{code}", get(handlers::game_page))
        .route("/ws/{code}", get(handlers::ws_handler))
        .with_state(state)
}
