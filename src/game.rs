use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use rand::Rng;
use tokio::sync::mpsc;

/// Room lifecycle states
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoomState {
    WaitingForPlayers,
    WaitingForReady,
    Countdown,
    Playing,
    GameOver,
}

/// Represents a game room
#[derive(Debug)]
pub struct Room {
    pub code: String,
    pub player_count: u8,
    pub state: RoomState,
    pub player1_ready: bool,
    pub player2_ready: bool,
    /// Per-player sender channels for broadcasting messages
    pub player_txs: Vec<mpsc::UnboundedSender<String>>,
}

impl Room {
    pub fn new(code: String) -> Self {
        Room {
            code,
            player_count: 0,
            state: RoomState::WaitingForPlayers,
            player1_ready: false,
            player2_ready: false,
            player_txs: Vec::new(),
        }
    }

    /// Register a player's sender channel. Returns the index (0 for player 1, 1 for player 2).
    pub fn add_player_tx(&mut self, tx: mpsc::UnboundedSender<String>) -> usize {
        let idx = self.player_txs.len();
        self.player_txs.push(tx);
        idx
    }

    /// Broadcast a message to all connected players
    pub fn broadcast(&self, msg: &str) {
        for tx in &self.player_txs {
            let _ = tx.send(msg.to_string());
        }
    }

    /// Mark a player as ready (player_num is 1 or 2). Returns true if both are now ready.
    pub fn set_player_ready(&mut self, player_num: u8) -> bool {
        match player_num {
            1 => self.player1_ready = true,
            2 => self.player2_ready = true,
            _ => {}
        }
        self.both_ready()
    }

    /// Check if both players are ready
    pub fn both_ready(&self) -> bool {
        self.player1_ready && self.player2_ready
    }

    /// Check if the room is currently in a playing state
    pub fn is_playing(&self) -> bool {
        self.state == RoomState::Playing
    }
}

/// Shared application state
#[derive(Debug, Clone)]
pub struct AppState {
    pub rooms: Arc<Mutex<HashMap<String, Room>>>,
}

impl AppState {
    pub fn new() -> Self {
        AppState {
            rooms: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

/// Generate a random 4-character alphanumeric room code (uppercase + digits)
pub fn generate_room_code() -> String {
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::rng();
    (0..4)
        .map(|_| {
            let idx = rng.random_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

/// Create a new room with a unique code, returning the code
pub fn create_room(state: &AppState) -> String {
    let mut rooms = state.rooms.lock().unwrap();
    loop {
        let code = generate_room_code();
        if !rooms.contains_key(&code) {
            rooms.insert(code.clone(), Room::new(code.clone()));
            return code;
        }
    }
}

/// Result of attempting to join a room
#[derive(Debug, PartialEq)]
pub enum JoinResult {
    /// Successfully joined as this player number (1 or 2)
    Joined(u8),
    /// Room is full
    RoomFull,
    /// Room not found
    NotFound,
}

/// Attempt to join a room, incrementing the player count
pub fn join_room(state: &AppState, code: &str) -> JoinResult {
    let mut rooms = state.rooms.lock().unwrap();
    match rooms.get_mut(code) {
        Some(room) => {
            if room.player_count >= 2 {
                JoinResult::RoomFull
            } else {
                room.player_count += 1;
                JoinResult::Joined(room.player_count)
            }
        }
        None => JoinResult::NotFound,
    }
}

/// Set the state of a room
pub fn set_room_state(state: &AppState, code: &str, room_state: RoomState) {
    let mut rooms = state.rooms.lock().unwrap();
    if let Some(room) = rooms.get_mut(code) {
        room.state = room_state;
    }
}

/// Decrement player count when a player leaves
pub fn leave_room(state: &AppState, code: &str) {
    let mut rooms = state.rooms.lock().unwrap();
    if let Some(room) = rooms.get_mut(code) {
        if room.player_count > 0 {
            room.player_count -= 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_room_code_length() {
        let code = generate_room_code();
        assert_eq!(code.len(), 4);
    }

    #[test]
    fn test_generate_room_code_charset() {
        let code = generate_room_code();
        for c in code.chars() {
            assert!(
                c.is_ascii_uppercase() || c.is_ascii_digit(),
                "Invalid character in room code: {c}"
            );
        }
    }

    #[test]
    fn test_generate_room_code_uniqueness() {
        // Generate many codes and check they're not all the same
        let codes: Vec<String> = (0..100).map(|_| generate_room_code()).collect();
        let unique: std::collections::HashSet<&String> = codes.iter().collect();
        assert!(unique.len() > 1, "All generated codes were identical");
    }

    #[test]
    fn test_create_room() {
        let state = AppState::new();
        let code = create_room(&state);
        assert_eq!(code.len(), 4);
        let rooms = state.rooms.lock().unwrap();
        assert!(rooms.contains_key(&code));
        assert_eq!(rooms[&code].player_count, 0);
    }

    #[test]
    fn test_join_room_first_player() {
        let state = AppState::new();
        let code = create_room(&state);
        let result = join_room(&state, &code);
        assert_eq!(result, JoinResult::Joined(1));
    }

    #[test]
    fn test_join_room_second_player() {
        let state = AppState::new();
        let code = create_room(&state);
        join_room(&state, &code);
        let result = join_room(&state, &code);
        assert_eq!(result, JoinResult::Joined(2));
    }

    #[test]
    fn test_join_room_full() {
        let state = AppState::new();
        let code = create_room(&state);
        join_room(&state, &code);
        join_room(&state, &code);
        let result = join_room(&state, &code);
        assert_eq!(result, JoinResult::RoomFull);
    }

    #[test]
    fn test_join_room_not_found() {
        let state = AppState::new();
        let result = join_room(&state, "ZZZZ");
        assert_eq!(result, JoinResult::NotFound);
    }

    #[test]
    fn test_leave_room() {
        let state = AppState::new();
        let code = create_room(&state);
        join_room(&state, &code);
        join_room(&state, &code);
        leave_room(&state, &code);
        // After leaving, should be able to join again
        let result = join_room(&state, &code);
        assert_eq!(result, JoinResult::Joined(2));
    }

    #[test]
    fn test_room_initial_state() {
        let room = Room::new("TEST".to_string());
        assert_eq!(room.state, RoomState::WaitingForPlayers);
        assert!(!room.player1_ready);
        assert!(!room.player2_ready);
    }

    #[test]
    fn test_set_player1_ready() {
        let mut room = Room::new("TEST".to_string());
        let both = room.set_player_ready(1);
        assert!(room.player1_ready);
        assert!(!room.player2_ready);
        assert!(!both);
    }

    #[test]
    fn test_set_player2_ready() {
        let mut room = Room::new("TEST".to_string());
        let both = room.set_player_ready(2);
        assert!(!room.player1_ready);
        assert!(room.player2_ready);
        assert!(!both);
    }

    #[test]
    fn test_both_players_ready() {
        let mut room = Room::new("TEST".to_string());
        room.set_player_ready(1);
        let both = room.set_player_ready(2);
        assert!(both);
        assert!(room.both_ready());
    }

    #[test]
    fn test_broadcast_sends_to_all_players() {
        let mut room = Room::new("TEST".to_string());
        let (tx1, mut rx1) = tokio::sync::mpsc::unbounded_channel();
        let (tx2, mut rx2) = tokio::sync::mpsc::unbounded_channel();
        room.add_player_tx(tx1);
        room.add_player_tx(tx2);

        room.broadcast(r#"{"type":"test"}"#);

        assert_eq!(rx1.try_recv().unwrap(), r#"{"type":"test"}"#);
        assert_eq!(rx2.try_recv().unwrap(), r#"{"type":"test"}"#);
    }

    #[test]
    fn test_room_state_transitions() {
        let mut room = Room::new("TEST".to_string());
        assert_eq!(room.state, RoomState::WaitingForPlayers);

        // Simulate both players joining
        room.state = RoomState::WaitingForReady;
        assert_eq!(room.state, RoomState::WaitingForReady);

        // Both ready -> countdown
        room.set_player_ready(1);
        room.set_player_ready(2);
        room.state = RoomState::Countdown;
        assert_eq!(room.state, RoomState::Countdown);

        // Countdown done -> playing
        room.state = RoomState::Playing;
        assert_eq!(room.state, RoomState::Playing);
    }
}
