use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use rand::Rng;
use tokio::sync::mpsc;

use crate::dictionary;

/// Room lifecycle states
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoomState {
    WaitingForPlayers,
    WaitingForReady,
    Countdown,
    Playing,
}

/// Per-player game state during a match
#[derive(Debug, Clone)]
pub struct PlayerState {
    /// Words from opponent that this player must type (defense queue)
    pub defense_queue: Vec<String>,
    /// Last word sent, for back-to-back duplicate prevention
    pub last_sent_word: Option<String>,
    /// Counter of words successfully sent to opponent
    pub words_sent: u32,
}

impl PlayerState {
    pub fn new() -> Self {
        PlayerState {
            defense_queue: Vec::new(),
            last_sent_word: None,
            words_sent: 0,
        }
    }
}

/// Result of attempting to submit a word
#[derive(Debug, PartialEq)]
pub enum WordSubmitResult {
    /// Word was valid and added to opponent's defense queue
    Sent { word: String },
    /// Word is not in the dictionary
    InvalidWord,
    /// Word is too short (less than 3 characters)
    TooShort,
    /// Word contains non-lowercase-letter characters
    InvalidChars,
    /// Same word as last sent (back-to-back duplicate)
    BackToBackDuplicate,
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
    /// Player 1 game state (initialized when game starts)
    pub player1_state: Option<PlayerState>,
    /// Player 2 game state (initialized when game starts)
    pub player2_state: Option<PlayerState>,
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
            player1_state: None,
            player2_state: None,
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

    /// Initialize player states when the game starts
    pub fn init_player_states(&mut self) {
        self.player1_state = Some(PlayerState::new());
        self.player2_state = Some(PlayerState::new());
    }

    /// Submit a word from a player. Validates and adds to opponent's defense queue.
    /// player_num is 1 or 2.
    pub fn submit_word(&mut self, player_num: u8, word: &str) -> WordSubmitResult {
        // Validate the word using dictionary module (checks length, chars, and dictionary)
        if let Some(reason) = dictionary::validate_word(word) {
            return match reason {
                "too_short" => WordSubmitResult::TooShort,
                "invalid_chars" => WordSubmitResult::InvalidChars,
                "not_in_dictionary" => WordSubmitResult::InvalidWord,
                _ => WordSubmitResult::InvalidWord,
            };
        }

        // Get the submitter's state to check for back-to-back duplicate
        let submitter_state = match player_num {
            1 => self.player1_state.as_mut(),
            2 => self.player2_state.as_mut(),
            _ => return WordSubmitResult::InvalidWord,
        };

        let submitter = submitter_state.expect("player state not initialized");

        // Check for back-to-back duplicate
        if submitter.last_sent_word.as_deref() == Some(word) {
            return WordSubmitResult::BackToBackDuplicate;
        }

        // Update submitter's state
        submitter.last_sent_word = Some(word.to_string());
        submitter.words_sent += 1;

        // Add to opponent's defense queue
        let opponent_state = match player_num {
            1 => self.player2_state.as_mut(),
            2 => self.player1_state.as_mut(),
            _ => unreachable!(),
        };

        let opponent = opponent_state.expect("opponent state not initialized");
        opponent.defense_queue.push(word.to_string());

        WordSubmitResult::Sent {
            word: word.to_string(),
        }
    }

    /// Get the defense queue length for a player
    pub fn queue_len(&self, player_num: u8) -> usize {
        match player_num {
            1 => self
                .player1_state
                .as_ref()
                .map(|s| s.defense_queue.len())
                .unwrap_or(0),
            2 => self
                .player2_state
                .as_ref()
                .map(|s| s.defense_queue.len())
                .unwrap_or(0),
            _ => 0,
        }
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
    fn test_player_state_new() {
        let state = PlayerState::new();
        assert!(state.defense_queue.is_empty());
        assert!(state.last_sent_word.is_none());
        assert_eq!(state.words_sent, 0);
    }

    #[test]
    fn test_init_player_states() {
        let mut room = Room::new("TEST".to_string());
        assert!(room.player1_state.is_none());
        assert!(room.player2_state.is_none());
        room.init_player_states();
        assert!(room.player1_state.is_some());
        assert!(room.player2_state.is_some());
    }

    #[test]
    fn test_submit_word_valid() {
        let mut room = Room::new("TEST".to_string());
        room.init_player_states();
        // "abacus" is in the EFF wordlist
        let result = room.submit_word(1, "abacus");
        assert_eq!(
            result,
            WordSubmitResult::Sent {
                word: "abacus".to_string()
            }
        );
        // Should be in player 2's defense queue
        assert_eq!(room.player2_state.as_ref().unwrap().defense_queue.len(), 1);
        assert_eq!(
            room.player2_state.as_ref().unwrap().defense_queue[0],
            "abacus"
        );
        // Player 1's stats should be updated
        assert_eq!(room.player1_state.as_ref().unwrap().words_sent, 1);
        assert_eq!(
            room.player1_state.as_ref().unwrap().last_sent_word,
            Some("abacus".to_string())
        );
    }

    #[test]
    fn test_submit_word_too_short() {
        let mut room = Room::new("TEST".to_string());
        room.init_player_states();
        assert_eq!(room.submit_word(1, "ab"), WordSubmitResult::TooShort);
        assert_eq!(room.submit_word(1, ""), WordSubmitResult::TooShort);
    }

    #[test]
    fn test_submit_word_invalid_chars() {
        let mut room = Room::new("TEST".to_string());
        room.init_player_states();
        assert_eq!(room.submit_word(1, "Hello"), WordSubmitResult::InvalidChars);
        assert_eq!(
            room.submit_word(1, "test123"),
            WordSubmitResult::InvalidChars
        );
    }

    #[test]
    fn test_submit_word_not_in_dictionary() {
        let mut room = Room::new("TEST".to_string());
        room.init_player_states();
        assert_eq!(room.submit_word(1, "zzzzzzz"), WordSubmitResult::InvalidWord);
    }

    #[test]
    fn test_submit_word_back_to_back_duplicate() {
        let mut room = Room::new("TEST".to_string());
        room.init_player_states();
        let result = room.submit_word(1, "abacus");
        assert_eq!(
            result,
            WordSubmitResult::Sent {
                word: "abacus".to_string()
            }
        );
        let result = room.submit_word(1, "abacus");
        assert_eq!(result, WordSubmitResult::BackToBackDuplicate);
    }

    #[test]
    fn test_submit_word_different_word_after_duplicate_check() {
        let mut room = Room::new("TEST".to_string());
        room.init_player_states();
        room.submit_word(1, "abacus");
        // Different word should work
        let result = room.submit_word(1, "abide");
        assert_eq!(
            result,
            WordSubmitResult::Sent {
                word: "abide".to_string()
            }
        );
        // Now sending "abacus" again should work (not back-to-back)
        let result = room.submit_word(1, "abacus");
        assert_eq!(
            result,
            WordSubmitResult::Sent {
                word: "abacus".to_string()
            }
        );
    }

    #[test]
    fn test_submit_word_player2_sends() {
        let mut room = Room::new("TEST".to_string());
        room.init_player_states();
        let result = room.submit_word(2, "abacus");
        assert_eq!(
            result,
            WordSubmitResult::Sent {
                word: "abacus".to_string()
            }
        );
        // Should be in player 1's defense queue
        assert_eq!(room.player1_state.as_ref().unwrap().defense_queue.len(), 1);
        assert_eq!(room.player2_state.as_ref().unwrap().words_sent, 1);
    }

    #[test]
    fn test_queue_len() {
        let mut room = Room::new("TEST".to_string());
        room.init_player_states();
        assert_eq!(room.queue_len(1), 0);
        assert_eq!(room.queue_len(2), 0);
        room.submit_word(1, "abacus");
        assert_eq!(room.queue_len(1), 0);
        assert_eq!(room.queue_len(2), 1);
        room.submit_word(1, "abide");
        assert_eq!(room.queue_len(2), 2);
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
