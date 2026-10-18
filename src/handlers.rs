use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Path, State, WebSocketUpgrade};
use axum::response::{Html, IntoResponse, Json};

use crate::game::{self, AppState, JoinResult, RoomState, WordSubmitResult};

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
  #attack-area {{ display: none; margin-top: 20px; }}
  #attack-area.visible {{ display: block; }}
  #word-input {{
    font-size: 1.5em; padding: 8px 16px; width: 300px;
    border: 2px solid #333; border-radius: 4px; text-align: center;
  }}
  #word-input.error {{ border-color: #e74c3c; }}
  #word-input.success {{ border-color: #2ecc71; }}
  .queue-display {{ font-size: 1.2em; margin: 10px 0; }}
  #feedback {{ min-height: 1.5em; margin-top: 10px; font-weight: bold; }}
  #feedback.error {{ color: #e74c3c; }}
  #feedback.success {{ color: #2ecc71; }}
</style>
</head>
<body>
<h1>Type Duel - Room {code}</h1>
<div id="status">Connecting...</div>
<button id="ready-btn" style="display:none;">Ready</button>
<div id="countdown-overlay"><span id="countdown-text"></span></div>
<div id="attack-area">
  <div class="queue-display">Opponent: <span id="opponent-queue">0</span> words</div>
  <div class="queue-display">Your queue: <span id="your-queue">0</span> words</div>
  <input type="text" id="word-input" placeholder="Type a word..." autocomplete="off" />
  <div id="feedback"></div>
</div>
<script>
  const code = "{code}";
  const protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
  const ws = new WebSocket(`${{protocol}}//${{location.host}}/ws/${{code}}`);
  const statusEl = document.getElementById('status');
  const readyBtn = document.getElementById('ready-btn');
  const overlay = document.getElementById('countdown-overlay');
  const countdownText = document.getElementById('countdown-text');
  const attackArea = document.getElementById('attack-area');
  const wordInput = document.getElementById('word-input');
  const feedbackEl = document.getElementById('feedback');
  const opponentQueueEl = document.getElementById('opponent-queue');
  const yourQueueEl = document.getElementById('your-queue');
  let feedbackTimeout = null;

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
      attackArea.classList.add('visible');
      wordInput.focus();
    }} else if (msg.type === 'room_full') {{
      statusEl.textContent = 'Room is full!';
    }} else if (msg.type === 'word_sent') {{
      showFeedback('Sent: ' + msg.word, 'success');
    }} else if (msg.type === 'word_rejected') {{
      const reasons = {{
        'not_in_dictionary': 'Not in dictionary',
        'too_short': 'Too short',
        'invalid_chars': 'Invalid characters',
        'back_to_back_duplicate': 'Duplicate word'
      }};
      showFeedback(reasons[msg.reason] || msg.reason, 'error');
    }} else if (msg.type === 'word_received') {{
      // Will be used for defense mode in Ticket 4
    }} else if (msg.type === 'queue_update') {{
      yourQueueEl.textContent = msg.your_queue;
      opponentQueueEl.textContent = msg.opponent_queue;
    }}
  }};

  function showFeedback(text, type) {{
    if (feedbackTimeout) clearTimeout(feedbackTimeout);
    feedbackEl.textContent = text;
    feedbackEl.className = type;
    wordInput.className = type;
    feedbackTimeout = setTimeout(() => {{
      feedbackEl.textContent = '';
      feedbackEl.className = '';
      wordInput.className = '';
    }}, 2000);
  }}

  wordInput.addEventListener('keydown', (e) => {{
    if (e.key === 'Enter' || e.key === ' ') {{
      e.preventDefault();
      const word = wordInput.value.trim().toLowerCase();
      if (word) {{
        ws.send(JSON.stringify({{"type": "submit_word", "word": word}}));
        wordInput.value = '';
      }}
    }}
  }});

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

    let msg_type = parsed.get("type").and_then(|t| t.as_str());

    match msg_type {
        Some("ready") => {
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
        Some("submit_word") => {
            let word = match parsed.get("word").and_then(|w| w.as_str()) {
                Some(w) => w.to_string(),
                None => return,
            };

            let mut rooms = state.rooms.lock().unwrap();
            if let Some(room) = rooms.get_mut(code) {
                if room.state != RoomState::Playing {
                    return;
                }

                let result = room.submit_word(player_num, &word);
                let player_idx = (player_num - 1) as usize;
                let opponent_idx = if player_num == 1 { 1 } else { 0 };

                match result {
                    WordSubmitResult::Sent { ref word } => {
                        // Send word_sent to submitter
                        if let Some(tx) = room.player_txs.get(player_idx) {
                            let _ = tx.send(
                                serde_json::json!({
                                    "type": "word_sent",
                                    "word": word
                                })
                                .to_string(),
                            );
                        }

                        // Send word_received to opponent
                        if let Some(tx) = room.player_txs.get(opponent_idx) {
                            let _ = tx.send(
                                serde_json::json!({
                                    "type": "word_received",
                                    "word": word
                                })
                                .to_string(),
                            );
                        }

                        // Send queue_update to both players (each gets their own perspective)
                        let p1_queue = room.queue_len(1);
                        let p2_queue = room.queue_len(2);

                        // Player 1's perspective
                        if let Some(tx) = room.player_txs.first() {
                            let _ = tx.send(
                                serde_json::json!({
                                    "type": "queue_update",
                                    "your_queue": p1_queue,
                                    "opponent_queue": p2_queue
                                })
                                .to_string(),
                            );
                        }

                        // Player 2's perspective
                        if let Some(tx) = room.player_txs.get(1) {
                            let _ = tx.send(
                                serde_json::json!({
                                    "type": "queue_update",
                                    "your_queue": p2_queue,
                                    "opponent_queue": p1_queue
                                })
                                .to_string(),
                            );
                        }
                    }
                    _ => {
                        // Send word_rejected to submitter only
                        let reason = match &result {
                            WordSubmitResult::InvalidWord => "not_in_dictionary",
                            WordSubmitResult::TooShort => "too_short",
                            WordSubmitResult::InvalidChars => "invalid_chars",
                            WordSubmitResult::BackToBackDuplicate => "back_to_back_duplicate",
                            WordSubmitResult::Sent { .. } => unreachable!(),
                        };

                        if let Some(tx) = room.player_txs.get(player_idx) {
                            let _ = tx.send(
                                serde_json::json!({
                                    "type": "word_rejected",
                                    "word": word,
                                    "reason": reason
                                })
                                .to_string(),
                            );
                        }
                    }
                }
            }
        }
        _ => {}
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

    // Transition to Playing and initialize player states
    let mut rooms = state.rooms.lock().unwrap();
    if let Some(room) = rooms.get_mut(code) {
        room.state = RoomState::Playing;
        room.init_player_states();
    }
}
