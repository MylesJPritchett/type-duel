use type_duel::app;

#[tokio::main]
async fn main() {
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    println!("Type Duel server running on http://localhost:3000");
    axum::serve(listener, app()).await.unwrap();
}
