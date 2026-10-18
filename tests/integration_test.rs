use futures::{SinkExt, StreamExt};
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

/// Helper: read the next JSON message from a WebSocket, with a timeout
async fn recv_json(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> serde_json::Value {
    let msg = tokio::time::timeout(std::time::Duration::from_secs(10), ws.next())
        .await
        .expect("timed out waiting for message")
        .unwrap()
        .unwrap();
    serde_json::from_str(&msg.into_text().unwrap()).unwrap()
}

/// Helper: send a JSON message over WebSocket
async fn send_json(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    value: serde_json::Value,
) {
    ws.send(Message::Text(value.to_string().into()))
        .await
        .unwrap();
}

#[tokio::test]
async fn test_ready_up_single_player_no_countdown() {
    let base_url = start_server().await;
    let code = create_room(&base_url).await;
    let ws_url = base_url.replace("http://", "ws://");

    // Player 1 connects
    let (mut ws1, _) = connect_async(format!("{ws_url}/ws/{code}")).await.unwrap();
    let msg = recv_json(&mut ws1).await;
    assert_eq!(msg["type"], "room_joined");

    // Player 2 connects
    let (mut ws2, _) = connect_async(format!("{ws_url}/ws/{code}")).await.unwrap();
    let _ = recv_json(&mut ws2).await; // room_joined for p2
    let _ = recv_json(&mut ws1).await; // player_joined for p1

    // Both should get waiting_for_ready
    let msg1 = recv_json(&mut ws1).await;
    assert_eq!(msg1["type"], "waiting_for_ready");
    let msg2 = recv_json(&mut ws2).await;
    assert_eq!(msg2["type"], "waiting_for_ready");

    // Only player 1 readies up
    send_json(&mut ws1, serde_json::json!({"type": "ready"})).await;

    // Should not receive countdown (only one player ready)
    let result = tokio::time::timeout(std::time::Duration::from_millis(500), ws1.next()).await;
    assert!(result.is_err(), "Should not receive any message when only one player is ready");

    ws1.close(None).await.unwrap();
    ws2.close(None).await.unwrap();
}

#[tokio::test]
async fn test_both_ready_triggers_countdown_and_game_start() {
    let base_url = start_server().await;
    let code = create_room(&base_url).await;
    let ws_url = base_url.replace("http://", "ws://");

    // Player 1 connects
    let (mut ws1, _) = connect_async(format!("{ws_url}/ws/{code}")).await.unwrap();
    let msg = recv_json(&mut ws1).await;
    assert_eq!(msg["type"], "room_joined");
    assert_eq!(msg["player"], 1);

    // Player 2 connects
    let (mut ws2, _) = connect_async(format!("{ws_url}/ws/{code}")).await.unwrap();
    let msg = recv_json(&mut ws2).await;
    assert_eq!(msg["type"], "room_joined");
    assert_eq!(msg["player"], 2);

    // Player 1 gets player_joined
    let msg = recv_json(&mut ws1).await;
    assert_eq!(msg["type"], "player_joined");

    // Both get waiting_for_ready
    let msg1 = recv_json(&mut ws1).await;
    assert_eq!(msg1["type"], "waiting_for_ready");
    let msg2 = recv_json(&mut ws2).await;
    assert_eq!(msg2["type"], "waiting_for_ready");

    // Both players ready up
    send_json(&mut ws1, serde_json::json!({"type": "ready"})).await;
    send_json(&mut ws2, serde_json::json!({"type": "ready"})).await;

    // Both should receive countdown 3, 2, 1, 0
    for expected_value in [3, 2, 1, 0] {
        let msg1 = recv_json(&mut ws1).await;
        assert_eq!(msg1["type"], "countdown");
        assert_eq!(msg1["value"], expected_value);

        let msg2 = recv_json(&mut ws2).await;
        assert_eq!(msg2["type"], "countdown");
        assert_eq!(msg2["value"], expected_value);
    }

    // Both should receive game_start
    let msg1 = recv_json(&mut ws1).await;
    assert_eq!(msg1["type"], "game_start");

    let msg2 = recv_json(&mut ws2).await;
    assert_eq!(msg2["type"], "game_start");

    ws1.close(None).await.unwrap();
    ws2.close(None).await.unwrap();
}

type WsStream = tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
>;

/// Helper: connect two players, ready up, wait for countdown + game_start.
/// Returns both WebSocket streams in the Playing state.
async fn setup_playing_game(base_url: &str) -> (WsStream, WsStream) {
    let code = create_room(base_url).await;
    let ws_url = base_url.replace("http://", "ws://");

    let (mut ws1, _) = connect_async(format!("{ws_url}/ws/{code}")).await.unwrap();
    let _ = recv_json(&mut ws1).await; // room_joined

    let (mut ws2, _) = connect_async(format!("{ws_url}/ws/{code}")).await.unwrap();
    let _ = recv_json(&mut ws2).await; // room_joined
    let _ = recv_json(&mut ws1).await; // player_joined

    // waiting_for_ready
    let _ = recv_json(&mut ws1).await;
    let _ = recv_json(&mut ws2).await;

    // Both ready up
    send_json(&mut ws1, serde_json::json!({"type": "ready"})).await;
    send_json(&mut ws2, serde_json::json!({"type": "ready"})).await;

    // Consume countdown 3,2,1,0
    for _ in 0..4 {
        let _ = recv_json(&mut ws1).await;
        let _ = recv_json(&mut ws2).await;
    }

    // game_start
    let msg1 = recv_json(&mut ws1).await;
    assert_eq!(msg1["type"], "game_start");
    let msg2 = recv_json(&mut ws2).await;
    assert_eq!(msg2["type"], "game_start");

    (ws1, ws2)
}

#[tokio::test]
async fn test_submit_word_valid() {
    let base_url = start_server().await;
    let (mut ws1, mut ws2) = setup_playing_game(&base_url).await;

    // Player 1 submits a valid word
    send_json(&mut ws1, serde_json::json!({"type": "submit_word", "word": "abacus"})).await;

    // Player 1 should get word_sent
    let msg = recv_json(&mut ws1).await;
    assert_eq!(msg["type"], "word_sent");
    assert_eq!(msg["word"], "abacus");

    // Player 2 should get word_received
    let msg = recv_json(&mut ws2).await;
    assert_eq!(msg["type"], "word_received");
    assert_eq!(msg["word"], "abacus");

    // Both should get queue_update
    let msg1 = recv_json(&mut ws1).await;
    assert_eq!(msg1["type"], "queue_update");
    assert_eq!(msg1["your_queue"], 0);
    assert_eq!(msg1["opponent_queue"], 1);

    let msg2 = recv_json(&mut ws2).await;
    assert_eq!(msg2["type"], "queue_update");
    assert_eq!(msg2["your_queue"], 1);
    assert_eq!(msg2["opponent_queue"], 0);

    ws1.close(None).await.unwrap();
    ws2.close(None).await.unwrap();
}

#[tokio::test]
async fn test_submit_word_rejected_not_in_dictionary() {
    let base_url = start_server().await;
    let (mut ws1, mut ws2) = setup_playing_game(&base_url).await;

    send_json(&mut ws1, serde_json::json!({"type": "submit_word", "word": "zzzzzzz"})).await;

    // Player 1 should get word_rejected
    let msg = recv_json(&mut ws1).await;
    assert_eq!(msg["type"], "word_rejected");
    assert_eq!(msg["word"], "zzzzzzz");
    assert_eq!(msg["reason"], "not_in_dictionary");

    // Player 2 should NOT get any message
    let result = tokio::time::timeout(std::time::Duration::from_millis(300), ws2.next()).await;
    assert!(result.is_err(), "Opponent should not receive rejected word");

    ws1.close(None).await.unwrap();
    ws2.close(None).await.unwrap();
}

#[tokio::test]
async fn test_submit_word_rejected_too_short() {
    let base_url = start_server().await;
    let (mut ws1, mut ws2) = setup_playing_game(&base_url).await;

    send_json(&mut ws1, serde_json::json!({"type": "submit_word", "word": "ab"})).await;

    let msg = recv_json(&mut ws1).await;
    assert_eq!(msg["type"], "word_rejected");
    assert_eq!(msg["reason"], "too_short");

    // Opponent gets nothing
    let result = tokio::time::timeout(std::time::Duration::from_millis(300), ws2.next()).await;
    assert!(result.is_err());

    ws1.close(None).await.unwrap();
    ws2.close(None).await.unwrap();
}

#[tokio::test]
async fn test_submit_word_rejected_invalid_chars() {
    let base_url = start_server().await;
    let (mut ws1, mut ws2) = setup_playing_game(&base_url).await;

    send_json(&mut ws1, serde_json::json!({"type": "submit_word", "word": "Hello"})).await;

    let msg = recv_json(&mut ws1).await;
    assert_eq!(msg["type"], "word_rejected");
    assert_eq!(msg["reason"], "invalid_chars");

    let result = tokio::time::timeout(std::time::Duration::from_millis(300), ws2.next()).await;
    assert!(result.is_err());

    ws1.close(None).await.unwrap();
    ws2.close(None).await.unwrap();
}

#[tokio::test]
async fn test_submit_word_back_to_back_duplicate() {
    let base_url = start_server().await;
    let (mut ws1, mut ws2) = setup_playing_game(&base_url).await;

    // First submission should succeed
    send_json(&mut ws1, serde_json::json!({"type": "submit_word", "word": "abacus"})).await;
    let _ = recv_json(&mut ws1).await; // word_sent
    let _ = recv_json(&mut ws2).await; // word_received
    let _ = recv_json(&mut ws1).await; // queue_update for p1
    let _ = recv_json(&mut ws2).await; // queue_update for p2

    // Same word again should be rejected
    send_json(&mut ws1, serde_json::json!({"type": "submit_word", "word": "abacus"})).await;
    let msg = recv_json(&mut ws1).await;
    assert_eq!(msg["type"], "word_rejected");
    assert_eq!(msg["reason"], "back_to_back_duplicate");

    ws1.close(None).await.unwrap();
    ws2.close(None).await.unwrap();
}

#[tokio::test]
async fn test_submit_word_not_allowed_before_playing() {
    let base_url = start_server().await;
    let code = create_room(&base_url).await;
    let ws_url = base_url.replace("http://", "ws://");

    let (mut ws1, _) = connect_async(format!("{ws_url}/ws/{code}")).await.unwrap();
    let _ = recv_json(&mut ws1).await; // room_joined

    // Try submitting a word before game starts (WaitingForPlayers state)
    send_json(&mut ws1, serde_json::json!({"type": "submit_word", "word": "abacus"})).await;

    // Should not receive any response
    let result = tokio::time::timeout(std::time::Duration::from_millis(300), ws1.next()).await;
    assert!(result.is_err(), "Should not accept words before game starts");

    ws1.close(None).await.unwrap();
}

#[tokio::test]
async fn test_submit_word_player2_can_send() {
    let base_url = start_server().await;
    let (mut ws1, mut ws2) = setup_playing_game(&base_url).await;

    // Player 2 submits a word
    send_json(&mut ws2, serde_json::json!({"type": "submit_word", "word": "zoom"})).await;

    // Player 2 should get word_sent
    let msg = recv_json(&mut ws2).await;
    assert_eq!(msg["type"], "word_sent");
    assert_eq!(msg["word"], "zoom");

    // Player 1 should get word_received
    let msg = recv_json(&mut ws1).await;
    assert_eq!(msg["type"], "word_received");
    assert_eq!(msg["word"], "zoom");

    // Both get queue_update
    let msg1 = recv_json(&mut ws1).await;
    assert_eq!(msg1["type"], "queue_update");
    assert_eq!(msg1["your_queue"], 1); // P1 now has a word in their queue
    assert_eq!(msg1["opponent_queue"], 0);

    let msg2 = recv_json(&mut ws2).await;
    assert_eq!(msg2["type"], "queue_update");
    assert_eq!(msg2["your_queue"], 0);
    assert_eq!(msg2["opponent_queue"], 1);

    ws1.close(None).await.unwrap();
    ws2.close(None).await.unwrap();
}

#[tokio::test]
async fn test_game_page_has_attack_ui() {
    let base_url = start_server().await;
    let code = create_room(&base_url).await;
    let res = reqwest::get(format!("{base_url}/game/{code}"))
        .await
        .unwrap();
    let body = res.text().await.unwrap();
    assert!(body.contains("word-input"), "Game page should have word input");
    assert!(body.contains("submit_word"), "Game page should send submit_word messages");
    assert!(body.contains("opponent-queue"), "Game page should show opponent queue");
    assert!(body.contains("attack-area"), "Game page should have attack area");
}

#[tokio::test]
async fn test_disconnect_during_playing_gives_opponent_win() {
    let base_url = start_server().await;
    let (ws1, mut ws2) = setup_playing_game(&base_url).await;

    // Player 1 disconnects
    drop(ws1);

    // Player 2 should receive game_over after timeout
    let msg = tokio::time::timeout(std::time::Duration::from_secs(10), recv_json(&mut ws2))
        .await
        .expect("timed out waiting for game_over");
    assert_eq!(msg["type"], "game_over");
    assert_eq!(msg["reason"], "opponent_disconnected");
    assert_eq!(msg["winner"], 2);
}

#[tokio::test]
async fn test_disconnect_player2_gives_player1_win() {
    let base_url = start_server().await;
    let (mut ws1, ws2) = setup_playing_game(&base_url).await;

    // Player 2 disconnects
    drop(ws2);

    // Player 1 should receive game_over after timeout
    let msg = tokio::time::timeout(std::time::Duration::from_secs(10), recv_json(&mut ws1))
        .await
        .expect("timed out waiting for game_over");
    assert_eq!(msg["type"], "game_over");
    assert_eq!(msg["reason"], "opponent_disconnected");
    assert_eq!(msg["winner"], 1);
}

#[tokio::test]
async fn test_word_completed_decrements_queue() {
    let base_url = start_server().await;
    let (mut ws1, mut ws2) = setup_playing_game(&base_url).await;

    // Player 1 sends a word to Player 2
    send_json(&mut ws1, serde_json::json!({"type": "submit_word", "word": "abacus"})).await;
    let _ = recv_json(&mut ws1).await; // word_sent
    let _ = recv_json(&mut ws2).await; // word_received
    let _ = recv_json(&mut ws1).await; // queue_update for p1
    let _ = recv_json(&mut ws2).await; // queue_update for p2

    // Player 2 completes the word
    send_json(&mut ws2, serde_json::json!({"type": "word_completed", "word": "abacus"})).await;

    // Both should get queue_update with decremented queue
    let msg2 = recv_json(&mut ws2).await;
    assert_eq!(msg2["type"], "queue_update");
    assert_eq!(msg2["your_queue"], 0);
    assert_eq!(msg2["opponent_queue"], 0);

    let msg1 = recv_json(&mut ws1).await;
    assert_eq!(msg1["type"], "queue_update");
    assert_eq!(msg1["your_queue"], 0);
    assert_eq!(msg1["opponent_queue"], 0);

    ws1.close(None).await.unwrap();
    ws2.close(None).await.unwrap();
}

#[tokio::test]
async fn test_full_attack_defend_cycle() {
    let base_url = start_server().await;
    let (mut ws1, mut ws2) = setup_playing_game(&base_url).await;

    // Player 1 sends a word
    send_json(&mut ws1, serde_json::json!({"type": "submit_word", "word": "abacus"})).await;
    let _ = recv_json(&mut ws1).await; // word_sent
    let msg = recv_json(&mut ws2).await; // word_received
    assert_eq!(msg["type"], "word_received");
    assert_eq!(msg["word"], "abacus");
    let _ = recv_json(&mut ws1).await; // queue_update
    let msg = recv_json(&mut ws2).await; // queue_update
    assert_eq!(msg["your_queue"], 1);

    // Player 2 completes the defense word
    send_json(&mut ws2, serde_json::json!({"type": "word_completed", "word": "abacus"})).await;

    // Queue should be empty now
    let msg = recv_json(&mut ws2).await;
    assert_eq!(msg["type"], "queue_update");
    assert_eq!(msg["your_queue"], 0);

    let msg = recv_json(&mut ws1).await;
    assert_eq!(msg["type"], "queue_update");
    assert_eq!(msg["opponent_queue"], 0);

    ws1.close(None).await.unwrap();
    ws2.close(None).await.unwrap();
}

#[tokio::test]
async fn test_multiple_words_completed_in_order() {
    let base_url = start_server().await;
    let (mut ws1, mut ws2) = setup_playing_game(&base_url).await;

    // Player 1 sends 3 words to Player 2
    let words = ["abacus", "abdomen", "abide"];
    for word in &words {
        send_json(&mut ws1, serde_json::json!({"type": "submit_word", "word": word})).await;
        let _ = recv_json(&mut ws1).await; // word_sent
        let _ = recv_json(&mut ws2).await; // word_received
        let _ = recv_json(&mut ws1).await; // queue_update
        let _ = recv_json(&mut ws2).await; // queue_update
    }

    // Player 2 completes them in order
    for (i, word) in words.iter().enumerate() {
        send_json(&mut ws2, serde_json::json!({"type": "word_completed", "word": word})).await;

        let msg2 = recv_json(&mut ws2).await;
        assert_eq!(msg2["type"], "queue_update");
        assert_eq!(msg2["your_queue"], (2 - i) as u64);

        let msg1 = recv_json(&mut ws1).await;
        assert_eq!(msg1["type"], "queue_update");
        assert_eq!(msg1["opponent_queue"], (2 - i) as u64);
    }

    ws1.close(None).await.unwrap();
    ws2.close(None).await.unwrap();
}

#[tokio::test]
async fn test_word_completed_wrong_word_ignored() {
    let base_url = start_server().await;
    let (mut ws1, mut ws2) = setup_playing_game(&base_url).await;

    // Player 1 sends a word
    send_json(&mut ws1, serde_json::json!({"type": "submit_word", "word": "abacus"})).await;
    let _ = recv_json(&mut ws1).await; // word_sent
    let _ = recv_json(&mut ws2).await; // word_received
    let _ = recv_json(&mut ws1).await; // queue_update
    let _ = recv_json(&mut ws2).await; // queue_update

    // Player 2 tries to complete with wrong word
    send_json(&mut ws2, serde_json::json!({"type": "word_completed", "word": "wrong"})).await;

    // No queue_update should be sent (wrong word)
    let result = tokio::time::timeout(std::time::Duration::from_millis(300), ws2.next()).await;
    assert!(result.is_err(), "Should not receive update for wrong word");

    ws1.close(None).await.unwrap();
    ws2.close(None).await.unwrap();
}

#[tokio::test]
async fn test_game_page_has_defense_display() {
    let base_url = start_server().await;
    let code = create_room(&base_url).await;
    let res = reqwest::get(format!("{base_url}/game/{code}"))
        .await
        .unwrap();
    let body = res.text().await.unwrap();
    assert!(body.contains("defense-area"), "Game page should have defense area");
    assert!(body.contains("defense-queue"), "Game page should have defense queue display");
    assert!(body.contains("word_completed"), "Game page should handle word_completed");
}

#[tokio::test]
async fn test_game_page_has_ready_button() {
    let base_url = start_server().await;
    let code = create_room(&base_url).await;
    let res = reqwest::get(format!("{base_url}/game/{code}"))
        .await
        .unwrap();
    let body = res.text().await.unwrap();
    assert!(body.contains("ready-btn"), "Game page should have a ready button");
    assert!(body.contains(r#""type":"ready""#), "Game page should send ready message");
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
