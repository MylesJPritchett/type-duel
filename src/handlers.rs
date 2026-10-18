use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Path, State, WebSocketUpgrade};
use axum::response::{Html, IntoResponse, Json};

use crate::game::{self, AppState, JoinResult};

/// GET / — Landing page with "Create Room" button
pub async fn index() -> Html<&'static str> {
    Html(
        r#"<!DOCTYPE html>
<html>
<head><title>Type Duel</title></head>
<body>
<h1>Type Duel</h1>
<button id="create-room">Create Room</button>
<script>
  document.getElementById('create-room').onclick = async () => {
    const res = await fetch('/create-room', { method: 'POST' });
    const data = await res.json();
    window.location.href = `/game/${data.code}`;
  };
</script>
</body>
</html>"#,
    )
}

/// POST /create-room — Create a new room and return its code
pub async fn create_room(State(state): State<AppState>) -> Json<serde_json::Value> {
    let code = game::create_room(&state);
    Json(serde_json::json!({ "code": code }))
}

/// GET /game/:code — Game page
pub async fn game_page(Path(code): Path<String>) -> Html<String> {
    Html(format!(
        r#"<!DOCTYPE html>
<html>
<head><title>Type Duel - Room {code}</title></head>
<body>
<h1>Type Duel - Room {code}</h1>
<div id="status">Connecting...</div>
<script>
  const code = "{code}";
  const protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
  const ws = new WebSocket(`${{protocol}}//${{location.host}}/ws/${{code}}`);
  const statusEl = document.getElementById('status');

  ws.onopen = () => {{
    statusEl.textContent = 'Connected, waiting for game data...';
  }};

  ws.onmessage = (event) => {{
    const msg = JSON.parse(event.data);
    if (msg.type === 'room_joined') {{
      statusEl.textContent = `Joined as Player ${{msg.player}}. Waiting for opponent...`;
    }} else if (msg.type === 'player_joined') {{
      statusEl.textContent = 'Opponent joined! Game starting soon...';
    }} else if (msg.type === 'room_full') {{
      statusEl.textContent = 'Room is full!';
    }}
  }};

  ws.onclose = () => {{
    if (statusEl.textContent.includes('full')) return;
    statusEl.textContent = 'Disconnected.';
  }};
</script>
</body>
</html>"#
    ))
}

/// WebSocket upgrade handler for /ws/:code
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    Path(code): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, code, state))
}

async fn handle_socket(mut socket: WebSocket, code: String, state: AppState) {
    let join_result = game::join_room(&state, &code);

    match join_result {
        JoinResult::Joined(player_num) => {
            let msg = serde_json::json!({
                "type": "room_joined",
                "player": player_num
            });
            if socket
                .send(Message::Text(msg.to_string().into()))
                .await
                .is_err()
            {
                game::leave_room(&state, &code);
                return;
            }

            // If this is player 2, notify player 1 that an opponent joined
            // For now we don't have a way to message the other player's socket directly.
            // We'll need to store sender handles. Let's add that.
            if player_num == 2 {
                // Notify the first player via the broadcast mechanism
                let rooms = state.rooms.lock().unwrap();
                if let Some(room) = rooms.get(&code) {
                    if let Some(ref tx) = room.notify_tx {
                        let _ = tx.send(serde_json::json!({"type": "player_joined"}).to_string());
                    }
                }
                drop(rooms);
            }

            // If this is player 1, set up a notification channel and wait
            if player_num == 1 {
                let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
                {
                    let mut rooms = state.rooms.lock().unwrap();
                    if let Some(room) = rooms.get_mut(&code) {
                        room.notify_tx = Some(tx);
                    }
                }

                // Wait for messages from the channel or the socket
                loop {
                    tokio::select! {
                        Some(notification) = rx.recv() => {
                            if socket
                                .send(Message::Text(notification.into()))
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                        msg = socket.recv() => {
                            match msg {
                                Some(Ok(_)) => {
                                    // For now, ignore client messages
                                }
                                _ => break,
                            }
                        }
                    }
                }
            } else {
                // Player 2: just hold the connection open
                loop {
                    match socket.recv().await {
                        Some(Ok(_)) => {}
                        _ => break,
                    }
                }
            }

            game::leave_room(&state, &code);
        }
        JoinResult::RoomFull => {
            let msg = serde_json::json!({"type": "room_full"});
            let _ = socket.send(Message::Text(msg.to_string().into())).await;
            // Drop socket to close the connection
        }
        JoinResult::NotFound => {
            // Drop socket to close the connection
        }
    }
}
