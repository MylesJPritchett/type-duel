use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Path, State, WebSocketUpgrade};
use axum::response::{Html, IntoResponse, Json};

use crate::game::{self, AppState, JoinResult, RoomState};

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
<head><title>Type Duel - Room {code}</title>
<style>
  body {{ font-family: sans-serif; text-align: center; margin-top: 40px; }}
  #ready-btn {{ font-size: 1.5em; padding: 10px 30px; cursor: pointer; }}
  #ready-btn:disabled {{ opacity: 0.5; cursor: not-allowed; }}
  #countdown-overlay {{
    display: none; position: fixed; top: 0; left: 0; width: 100%; height: 100%;
    background: rgba(0,0,0,0.7); color: white; font-size: 5em;
    justify-content: center; align-items: center; z-index: 100;
  }}
  #countdown-overlay.visible {{ display: flex; }}
</style>
</head>
<body>
<h1>Type Duel - Room {code}</h1>
<div id="status">Connecting...</div>
<button id="ready-btn" style="display:none;">Ready</button>
<div id="countdown-overlay"><span id="countdown-text"></span></div>
<script>
  const code = "{code}";
  const protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
  const ws = new WebSocket(`${{protocol}}//${{location.host}}/ws/${{code}}`);
  const statusEl = document.getElementById('status');
  const readyBtn = document.getElementById('ready-btn');
  const overlay = document.getElementById('countdown-overlay');
  const countdownText = document.getElementById('countdown-text');

  ws.onopen = () => {{
    statusEl.textContent = 'Connected, waiting for game data...';
  }};

  readyBtn.onclick = () => {{
    ws.send(JSON.stringify({{"type":"ready"}}));
    readyBtn.disabled = true;
    readyBtn.textContent = 'Waiting...';
  }};

  ws.onmessage = (event) => {{
    const msg = JSON.parse(event.data);
    if (msg.type === 'room_joined') {{
      statusEl.textContent = `Joined as Player ${{msg.player}}. Waiting for opponent...`;
    }} else if (msg.type === 'player_joined') {{
      statusEl.textContent = 'Opponent joined!';
    }} else if (msg.type === 'waiting_for_ready') {{
      statusEl.textContent = 'Both players connected. Click Ready!';
      readyBtn.style.display = 'inline-block';
    }} else if (msg.type === 'countdown') {{
      overlay.classList.add('visible');
      countdownText.textContent = msg.value === 0 ? 'GO!' : msg.value;
    }} else if (msg.type === 'game_start') {{
      overlay.classList.remove('visible');
      readyBtn.style.display = 'none';
      statusEl.textContent = 'Game in progress!';
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
            // Send room_joined to this player
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

            // Set up this player's notification channel
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();

            {
                let mut rooms = state.rooms.lock().unwrap();
                if let Some(room) = rooms.get_mut(&code) {
                    room.add_player_tx(tx);
                }
            }

            // If this is player 2, notify player 1 and transition to WaitingForReady
            if player_num == 2 {
                let mut rooms = state.rooms.lock().unwrap();
                if let Some(room) = rooms.get_mut(&code) {
                    // Notify player 1 that opponent joined
                    if let Some(p1_tx) = room.player_txs.first() {
                        let _ = p1_tx
                            .send(serde_json::json!({"type": "player_joined"}).to_string());
                    }
                    // Transition to WaitingForReady and broadcast
                    room.state = RoomState::WaitingForReady;
                    room.broadcast(
                        &serde_json::json!({"type": "waiting_for_ready"}).to_string(),
                    );
                }
                drop(rooms);
            }

            // Both players run the same select! loop
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
                            Some(Ok(Message::Text(text))) => {
                                handle_client_message(&state, &code, player_num, &text);
                            }
                            Some(Ok(_)) => {
                                // Ignore non-text messages
                            }
                            _ => break,
                        }
                    }
                }
            }

            game::leave_room(&state, &code);
        }
        JoinResult::RoomFull => {
            let msg = serde_json::json!({"type": "room_full"});
            let _ = socket.send(Message::Text(msg.to_string().into())).await;
        }
        JoinResult::NotFound => {
            // Drop socket to close the connection
        }
    }
}

fn handle_client_message(state: &AppState, code: &str, player_num: u8, text: &str) {
    let parsed: serde_json::Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => return,
    };

    if parsed.get("type").and_then(|t| t.as_str()) == Some("ready") {
        let mut rooms = state.rooms.lock().unwrap();
        if let Some(room) = rooms.get_mut(code) {
            if room.state != RoomState::WaitingForReady {
                return;
            }
            let both_ready = room.set_player_ready(player_num);
            if both_ready {
                room.state = RoomState::Countdown;
                // Collect senders to use outside the lock
                let senders: Vec<_> = room.player_txs.iter().cloned().collect();
                drop(rooms);

                // Spawn countdown task
                let state_clone = state.clone();
                let code_owned = code.to_string();
                tokio::spawn(async move {
                    run_countdown(&senders, &state_clone, &code_owned).await;
                });
            }
        }
    }
}

async fn run_countdown(
    senders: &[tokio::sync::mpsc::UnboundedSender<String>],
    state: &AppState,
    code: &str,
) {
    for value in [3, 2, 1, 0] {
        let msg = serde_json::json!({"type": "countdown", "value": value}).to_string();
        for tx in senders {
            let _ = tx.send(msg.clone());
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }

    // Send game_start
    let msg = serde_json::json!({"type": "game_start"}).to_string();
    for tx in senders {
        let _ = tx.send(msg.clone());
    }

    // Transition to Playing
    let mut rooms = state.rooms.lock().unwrap();
    if let Some(room) = rooms.get_mut(code) {
        room.state = RoomState::Playing;
    }
}
