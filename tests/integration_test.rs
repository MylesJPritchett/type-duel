use futures::StreamExt;
use tokio::net::TcpListener;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

/// Start the server on a random port, return the base URL
async fn start_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, type_duel::app()).await.unwrap();
    });
    format!("http://{addr}")
}

/// Helper: create a room via POST and return the code
async fn create_room(base_url: &str) -> String {
    let client = reqwest::Client::new();
    let res = client
        .post(format!("{base_url}/create-room"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let body: serde_json::Value = res.json().await.unwrap();
    body["code"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn test_landing_page() {
    let base_url = start_server().await;
    let res = reqwest::get(&base_url).await.unwrap();
    assert_eq!(res.status(), 200);
    let body = res.text().await.unwrap();
    assert!(body.contains("Create Room"));
    assert!(body.contains("create-room"));
}

#[tokio::test]
async fn test_create_room_returns_code() {
    let base_url = start_server().await;
    let code = create_room(&base_url).await;
    assert_eq!(code.len(), 4);
    for c in code.chars() {
        assert!(c.is_ascii_uppercase() || c.is_ascii_digit());
    }
}

#[tokio::test]
async fn test_game_page() {
    let base_url = start_server().await;
    let code = create_room(&base_url).await;
    let res = reqwest::get(format!("{base_url}/game/{code}"))
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let body = res.text().await.unwrap();
    assert!(body.contains(&code));
    assert!(body.contains("WebSocket"));
}

#[tokio::test]
async fn test_websocket_player_join() {
    let base_url = start_server().await;
    let code = create_room(&base_url).await;

    let ws_url = base_url.replace("http://", "ws://");
    let (mut ws, _) = connect_async(format!("{ws_url}/ws/{code}"))
        .await
        .expect("Failed to connect WebSocket");

    // Should receive room_joined with player 1
    let msg = ws.next().await.unwrap().unwrap();
    let text = msg.into_text().unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(json["type"], "room_joined");
    assert_eq!(json["player"], 1);

    ws.close(None).await.unwrap();
}

#[tokio::test]
async fn test_websocket_two_players() {
    let base_url = start_server().await;
    let code = create_room(&base_url).await;

    let ws_url = base_url.replace("http://", "ws://");

    // Player 1 connects
    let (mut ws1, _) = connect_async(format!("{ws_url}/ws/{code}"))
        .await
        .expect("P1 failed to connect");

    let msg = ws1.next().await.unwrap().unwrap();
    let json: serde_json::Value = serde_json::from_str(&msg.into_text().unwrap()).unwrap();
    assert_eq!(json["type"], "room_joined");
    assert_eq!(json["player"], 1);

    // Player 2 connects
    let (mut ws2, _) = connect_async(format!("{ws_url}/ws/{code}"))
        .await
        .expect("P2 failed to connect");

    let msg = ws2.next().await.unwrap().unwrap();
    let json: serde_json::Value = serde_json::from_str(&msg.into_text().unwrap()).unwrap();
    assert_eq!(json["type"], "room_joined");
    assert_eq!(json["player"], 2);

    // Player 1 should receive player_joined notification
    let msg = ws1.next().await.unwrap().unwrap();
    let json: serde_json::Value = serde_json::from_str(&msg.into_text().unwrap()).unwrap();
    assert_eq!(json["type"], "player_joined");

    ws1.close(None).await.unwrap();
    ws2.close(None).await.unwrap();
}

#[tokio::test]
async fn test_websocket_room_full() {
    let base_url = start_server().await;
    let code = create_room(&base_url).await;

    let ws_url = base_url.replace("http://", "ws://");

    // Player 1
    let (mut ws1, _) = connect_async(format!("{ws_url}/ws/{code}"))
        .await
        .unwrap();
    let _ = ws1.next().await; // consume room_joined

    // Player 2
    let (mut ws2, _) = connect_async(format!("{ws_url}/ws/{code}"))
        .await
        .unwrap();
    let _ = ws2.next().await; // consume room_joined

    // Consume player_joined on ws1
    let _ = ws1.next().await;

    // Player 3 — should get room_full
    let (mut ws3, _) = connect_async(format!("{ws_url}/ws/{code}"))
        .await
        .unwrap();

    let msg = ws3.next().await.unwrap().unwrap();
    let json: serde_json::Value = serde_json::from_str(&msg.into_text().unwrap()).unwrap();
    assert_eq!(json["type"], "room_full");

    // Connection should be closed after room_full
    // The server drops the socket, which may manifest as a Close frame, None, or an error
    let next = ws3.next().await;
    match &next {
        None => {} // stream ended
        Some(Ok(Message::Close(_))) => {} // close frame received
        Some(Err(_)) => {} // connection error (reset)
        other => panic!("Expected connection to close after room_full, got: {other:?}"),
    }

    ws1.close(None).await.unwrap();
    ws2.close(None).await.unwrap();
}
