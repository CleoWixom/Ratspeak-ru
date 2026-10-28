//! Battleship: a two-player LRGP game with hidden fleets.
//!
//! Unlike the built-in perfect-information games, each player's fleet is
//! secret, so the peers cannot simply replay each other's moves. Fairness is
//! enforced with a commit-reveal scheme:
//!
//! * After the challenge is accepted each player secretly places five ships
//!   and sends only `H = SHA-256(domain || session || player || fleet || salt)`.
//! * Every shot is answered (`m` miss, `h` hit, `s` sunk + ship index) inside
//!   the answerer's next shot message, so a round trip costs one envelope.
//! * When a fleet is fully sunk its owner reveals `fleet` and `salt`. The
//!   winner checks the commitment, the fleet shape, and that every answer
//!   given during the game matches the revealed fleet, then reveals in turn
//!   so the loser can run the same checks. A failed check ends the game in
//!   favor of the verifier.
//!
//! This does not prevent a dishonest peer from lying mid-game (nothing can
//! without a trusted third party) but it makes every lie detectable at the
//! end, and answers that cannot match *any* legal fleet (more than 17 hits,
//! a "sunk" ship without enough hits, ...) are caught immediately.
//!
//! The wire carries only short keys so every envelope stays far below the
//! 200-byte LRGP budget; a fleet is five bytes (`start * 2 + vertical`).
//! The local fleet and salt live in the session metadata of the owner only
//! and are never serialized into an envelope before the reveal.
//!
//! Wire payloads (all under the standard `move` command):
//!
//! | `k` | meaning | keys |
//! |-----|---------|------|
//! | `c` | commit | `k`, `h` (bin32) |
//! | `f` | fire (+ answer to the previous shot) | `k`, `n`, `c`, and `h` on `n == 1`, or `a` (+ `s` when `a == "s"`) on `n > 1` |
//! | `r` | reveal | `k`, `n`, `b` (bin5), `z` (bin16); the loser's reveal also carries `a = "s"` and `s` |

use std::collections::HashMap;
use std::sync::Mutex;

use lrgp::app_base::{AppManifest, GameApp, IncomingResult, OutgoingResult};
use lrgp::constants::*;
use lrgp::envelope::{value_as_str, value_as_u64};
use lrgp::errors::LrgpError;
use lrgp::router::LrgpRouter;
use lrgp::session::{Session, SessionStateMachine};
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};

pub(crate) const APP_ID: &str = "battleship";
pub const APP_VERSION: u32 = 1;
pub const GRID: usize = 10;
pub const CELLS: usize = GRID * GRID;
/// Carrier, Battleship, Cruiser, Submarine, Destroyer.
pub const SHIP_LENGTHS: [usize; 5] = [5, 4, 3, 3, 2];
pub const SHIP_CELLS: usize = 17;
const SALT_BYTES: usize = 16;
const COMMIT_BYTES: usize = 32;
const COMMIT_DOMAIN: &[u8] = b"ratspeak.battleship.v1";
const WATER: u8 = 0xFF;

const PHASE_SETUP: &str = "setup";
const PHASE_BATTLE: &str = "battle";
const PHASE_REVEAL: &str = "reveal";
const PHASE_DONE: &str = "done";

/// Build the Ratspeak game registry: every LRGP built-in plus Battleship.
///
/// Ratspeak owns this extra game; the upstream LRGP built-in list is not
/// modified. Every place that needs a router must use this function so the
/// set of games cannot drift between live dispatch, admission and hydration.
pub(crate) fn lrgp_router() -> LrgpRouter {
    let router = LrgpRouter::with_builtin_apps();
    router.register(Box::new(BattleshipApp::new()));
    router
}

// ---------------------------------------------------------------------------
// Fleet geometry and commitment
// ---------------------------------------------------------------------------

/// Decode a five-byte fleet into a board of ship indices (`WATER` elsewhere).
/// Ships may touch but never overlap or leave the grid.
pub fn layout_from_fleet(fleet: &[u8]) -> Option<[u8; CELLS]> {
    if fleet.len() != SHIP_LENGTHS.len() {
        return None;
    }
    let mut board = [WATER; CELLS];
    for (index, encoded) in fleet.iter().enumerate() {
        let start = usize::from(encoded / 2);
        let vertical = encoded % 2 == 1;
        if start >= CELLS {
            return None;
        }
        let length = SHIP_LENGTHS[index];
        let (row, column) = (start / GRID, start % GRID);
        let fits = if vertical {
            row + length <= GRID
        } else {
            column + length <= GRID
        };
        if !fits {
            return None;
        }
        for step in 0..length {
            let cell = if vertical {
                start + step * GRID
            } else {
                start + step
            };
            if board[cell] != WATER {
                return None;
            }
            board[cell] = index as u8;
        }
    }
    Some(board)
}

fn board_string(layout: &[u8; CELLS]) -> String {
    layout
        .iter()
        .map(|id| {
            if *id == WATER {
                '_'
            } else {
                char::from(b'0' + id)
            }
        })
        .collect()
}

fn commitment(session_id: &str, player: &str, fleet: &[u8], salt: &[u8]) -> [u8; COMMIT_BYTES] {
    let mut hasher = Sha256::new();
    hasher.update(COMMIT_DOMAIN);
    hasher.update([0]);
    hasher.update(session_id.as_bytes());
    hasher.update([0]);
    hasher.update(player.as_bytes());
    hasher.update([0]);
    hasher.update(fleet);
    hasher.update(salt);
    hasher.finalize().into()
}

fn random_salt() -> [u8; SALT_BYTES] {
    *uuid::Uuid::new_v4().as_bytes()
}

fn cell_label(cell: usize) -> String {
    format!(
        "{}{}",
        char::from(b'A' + (cell % GRID) as u8),
        cell / GRID + 1
    )
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

fn gen_session_id() -> String {
    lrgp::protocol::generate_session_id()
}

fn error_result(code: &str, msg: &str) -> IncomingResult {
    IncomingResult {
        session: None,
        emit: None,
        error: Some(HashMap::from([
            ("code".into(), JsonValue::String(code.into())),
            ("msg".into(), JsonValue::String(msg.into())),
        ])),
    }
}

fn emit_event(event_type: &str, session_id: &str, from: &str) -> HashMap<String, JsonValue> {
    HashMap::from([
        ("type".into(), JsonValue::String(event_type.into())),
        ("session_id".into(), JsonValue::String(session_id.into())),
        ("app_id".into(), JsonValue::String(APP_ID.into())),
        ("from".into(), JsonValue::String(from.into())),
    ])
}

fn exact_keys(payload: &HashMap<String, rmpv::Value>, expected: &[&str]) -> bool {
    payload.len() == expected.len() && payload.keys().all(|key| expected.contains(&key.as_str()))
}

fn value_as_bytes(value: &rmpv::Value) -> Option<&[u8]> {
    match value {
        rmpv::Value::Binary(bytes) => Some(bytes.as_slice()),
        _ => None,
    }
}

fn rmpv_to_json(value: &rmpv::Value) -> JsonValue {
    match value {
        rmpv::Value::Nil => JsonValue::Null,
        rmpv::Value::Boolean(v) => JsonValue::Bool(*v),
        rmpv::Value::Integer(v) => v
            .as_u64()
            .map(|n| JsonValue::Number(n.into()))
            .or_else(|| v.as_i64().map(|n| JsonValue::Number(n.into())))
            .unwrap_or(JsonValue::Null),
        rmpv::Value::F32(v) => serde_json::json!(*v),
        rmpv::Value::F64(v) => serde_json::json!(*v),
        rmpv::Value::String(v) => JsonValue::String(v.as_str().unwrap_or("").into()),
        rmpv::Value::Binary(v) => JsonValue::String(hex::encode(v)),
        rmpv::Value::Array(v) => JsonValue::Array(v.iter().map(rmpv_to_json).collect()),
        rmpv::Value::Map(v) => JsonValue::Object(
            v.iter()
                .filter_map(|(key, value)| Some((key.as_str()?.to_string(), rmpv_to_json(value))))
                .collect(),
        ),
        rmpv::Value::Ext(_, _) => JsonValue::Null,
    }
}

fn other_player(session: &Session, player: &str) -> Option<String> {
    if player == session.identity_id {
        (!session.contact_hash.is_empty()).then(|| session.contact_hash.clone())
    } else if player == session.contact_hash {
        (!session.identity_id.is_empty()).then(|| session.identity_id.clone())
    } else {
        None
    }
}

fn session_to_json(session: &Session) -> HashMap<String, JsonValue> {
    HashMap::from([
        (
            "session_id".into(),
            JsonValue::String(session.session_id.clone()),
        ),
        (
            "identity_id".into(),
            JsonValue::String(session.identity_id.clone()),
        ),
        ("app_id".into(), JsonValue::String(session.app_id.clone())),
        (
            "app_version".into(),
            JsonValue::Number((session.app_version as u64).into()),
        ),
        (
            "contact_hash".into(),
            JsonValue::String(session.contact_hash.clone()),
        ),
        (
            "initiator".into(),
            JsonValue::String(session.initiator.clone()),
        ),
        ("status".into(), JsonValue::String(session.status.clone())),
        (
            "metadata".into(),
            JsonValue::Object(session.metadata.clone().into_iter().collect()),
        ),
        ("unread".into(), JsonValue::Number(session.unread.into())),
        ("created_at".into(), serde_json::json!(session.created_at)),
        ("updated_at".into(), serde_json::json!(session.updated_at)),
        (
            "last_action_at".into(),
            serde_json::json!(session.last_action_at),
        ),
    ])
}

// ---------------------------------------------------------------------------
// Typed view of the session metadata
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
struct LogEntry {
    cell: usize,
    /// `m`, `h` or `s`.
    answer: u8,
    /// Ship index for `s`, otherwise `-1`.
    ship: i64,
}

/// Everything a participant knows about one game. `my_*` fields are private
/// to this device and never leave it before the reveal.
#[derive(Clone, Debug)]
struct Game {
    phase: String,
    first_turn: String,
    turn: String,
    move_count: u64,
    my_fleet: Vec<u8>,
    my_salt: Vec<u8>,
    my_commit: Vec<u8>,
    their_commit: Vec<u8>,
    /// Opponent shots at my board: `_` untouched, `m` miss, `h` hit.
    shots_in: Vec<u8>,
    /// My shots at the opponent: `_` untouched, `p` awaiting answer, `m`, `h`.
    shots_out: Vec<u8>,
    out_log: Vec<LogEntry>,
    /// Answer I owe for the opponent's last shot: `0`, `m`, `h` or `s`.
    owe: u8,
    owe_ship: i64,
    my_sunk: Vec<u8>,
    their_sunk: Vec<u8>,
    owes_reveal: bool,
    await_reveal: bool,
    their_board: String,
    winner: String,
    terminal: String,
    cheat_reason: String,
    last_in: Option<usize>,
    last_out: Option<usize>,
}

fn meta_get<'a>(meta: &'a HashMap<String, JsonValue>, key: &str) -> Result<&'a JsonValue, String> {
    meta.get(key)
        .ok_or_else(|| format!("metadata is missing {key}"))
}

fn meta_string(meta: &HashMap<String, JsonValue>, key: &str) -> Result<String, String> {
    meta_get(meta, key)?
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| format!("{key} must be a string"))
}

fn meta_hex(
    meta: &HashMap<String, JsonValue>,
    key: &str,
    length: usize,
) -> Result<Vec<u8>, String> {
    let text = meta_string(meta, key)?;
    if text.is_empty() {
        return Ok(Vec::new());
    }
    let bytes = hex::decode(&text).map_err(|_| format!("{key} must be hexadecimal"))?;
    if bytes.len() != length {
        return Err(format!("{key} has the wrong length"));
    }
    Ok(bytes)
}

fn meta_bytes_array(meta: &HashMap<String, JsonValue>, key: &str) -> Result<Vec<u8>, String> {
    meta_get(meta, key)?
        .as_array()
        .ok_or_else(|| format!("{key} must be an array"))?
        .iter()
        .map(|value| {
            value
                .as_u64()
                .and_then(|n| u8::try_from(n).ok())
                .ok_or_else(|| format!("{key} must contain bytes"))
        })
        .collect()
}

fn meta_shots(
    meta: &HashMap<String, JsonValue>,
    key: &str,
    allowed: &[u8],
) -> Result<Vec<u8>, String> {
    let text = meta_string(meta, key)?;
    let bytes = text.into_bytes();
    if bytes.len() != CELLS || bytes.iter().any(|b| !allowed.contains(b)) {
        return Err(format!("{key} is not a valid shot grid"));
    }
    Ok(bytes)
}

fn meta_cell(meta: &HashMap<String, JsonValue>, key: &str) -> Result<Option<usize>, String> {
    match meta_get(meta, key)? {
        JsonValue::Null => Ok(None),
        value => value
            .as_u64()
            .filter(|cell| *cell < CELLS as u64)
            .map(|cell| Some(cell as usize))
            .ok_or_else(|| format!("{key} must be a cell index or null")),
    }
}

impl Game {
    fn new(first_turn: &str) -> Self {
        Self {
            phase: PHASE_SETUP.into(),
            first_turn: first_turn.into(),
            turn: String::new(),
            move_count: 0,
            my_fleet: Vec::new(),
            my_salt: Vec::new(),
            my_commit: Vec::new(),
            their_commit: Vec::new(),
            shots_in: vec![b'_'; CELLS],
            shots_out: vec![b'_'; CELLS],
            out_log: Vec::new(),
            owe: 0,
            owe_ship: -1,
            my_sunk: Vec::new(),
            their_sunk: Vec::new(),
            owes_reveal: false,
            await_reveal: false,
            their_board: String::new(),
            winner: String::new(),
            terminal: String::new(),
            cheat_reason: String::new(),
            last_in: None,
            last_out: None,
        }
    }

    fn load(session: &Session) -> Result<Self, String> {
        let meta = &session.metadata;
        let phase = meta_string(meta, "phase")?;
        if !matches!(
            phase.as_str(),
            PHASE_SETUP | PHASE_BATTLE | PHASE_REVEAL | PHASE_DONE
        ) {
            return Err("phase is not recognized".into());
        }
        let owe = match meta_string(meta, "owe")?.as_str() {
            "" => 0,
            "m" => b'm',
            "h" => b'h',
            "s" => b's',
            _ => return Err("owe is not recognized".into()),
        };
        let out_log = meta_get(meta, "out_log")?
            .as_array()
            .ok_or_else(|| "out_log must be an array".to_string())?
            .iter()
            .map(|entry| {
                let parts = entry.as_array().filter(|parts| parts.len() == 3);
                let cell = parts
                    .and_then(|parts| parts[0].as_u64())
                    .filter(|cell| *cell < CELLS as u64);
                let answer = parts
                    .and_then(|parts| parts[1].as_str())
                    .and_then(|text| text.bytes().next().filter(|_| text.len() == 1))
                    .filter(|b| matches!(b, b'm' | b'h' | b's'));
                let ship = parts.and_then(|parts| parts[2].as_i64());
                match (cell, answer, ship) {
                    (Some(cell), Some(answer), Some(ship)) => Ok(LogEntry {
                        cell: cell as usize,
                        answer,
                        ship,
                    }),
                    _ => Err("out_log contains an invalid entry".to_string()),
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            phase,
            first_turn: meta_string(meta, "first_turn")?,
            turn: meta_string(meta, "turn")?,
            move_count: meta_get(meta, "move_count")?
                .as_u64()
                .ok_or_else(|| "move_count must be an integer".to_string())?,
            my_fleet: meta_bytes_array(meta, "my_fleet")?,
            my_salt: meta_hex(meta, "my_salt", SALT_BYTES)?,
            my_commit: meta_hex(meta, "my_commit", COMMIT_BYTES)?,
            their_commit: meta_hex(meta, "their_commit", COMMIT_BYTES)?,
            shots_in: meta_shots(meta, "shots_in", b"_mh")?,
            shots_out: meta_shots(meta, "shots_out", b"_pmh")?,
            out_log,
            owe,
            owe_ship: meta_get(meta, "owe_ship")?
                .as_i64()
                .filter(|ship| (-1..5).contains(ship))
                .ok_or_else(|| "owe_ship is not valid".to_string())?,
            my_sunk: meta_bytes_array(meta, "my_sunk")?,
            their_sunk: meta_bytes_array(meta, "their_sunk")?,
            owes_reveal: meta_get(meta, "owes_reveal")?
                .as_bool()
                .ok_or_else(|| "owes_reveal must be a boolean".to_string())?,
            await_reveal: meta_get(meta, "await_reveal")?
                .as_bool()
                .ok_or_else(|| "await_reveal must be a boolean".to_string())?,
            their_board: meta_string(meta, "their_board")?,
            winner: meta_string(meta, "winner")?,
            terminal: meta_string(meta, "terminal")?,
            cheat_reason: meta_string(meta, "cheat_reason")?,
            last_in: meta_cell(meta, "last_in")?,
            last_out: meta_cell(meta, "last_out")?,
        })
    }

    fn store(&self, session: &mut Session) {
        let number = |value: u64| JsonValue::Number(value.into());
        let text = |value: &str| JsonValue::String(value.into());
        let cell = |value: Option<usize>| value.map_or(JsonValue::Null, |c| number(c as u64));
        let bytes = |values: &[u8]| {
            JsonValue::Array(values.iter().map(|b| number(u64::from(*b))).collect())
        };
        let my_board = layout_from_fleet(&self.my_fleet)
            .map(|layout| board_string(&layout))
            .unwrap_or_default();
        let owe = if self.owe == 0 {
            String::new()
        } else {
            char::from(self.owe).to_string()
        };
        let log = JsonValue::Array(
            self.out_log
                .iter()
                .map(|entry| {
                    JsonValue::Array(vec![
                        number(entry.cell as u64),
                        text(&char::from(entry.answer).to_string()),
                        JsonValue::Number(entry.ship.into()),
                    ])
                })
                .collect(),
        );
        let pairs: Vec<(&str, JsonValue)> = vec![
            ("phase", text(&self.phase)),
            ("first_turn", text(&self.first_turn)),
            ("turn", text(&self.turn)),
            ("move_count", number(self.move_count)),
            ("my_fleet", bytes(&self.my_fleet)),
            ("my_board", text(&my_board)),
            ("my_salt", text(&hex::encode(&self.my_salt))),
            ("my_commit", text(&hex::encode(&self.my_commit))),
            ("their_commit", text(&hex::encode(&self.their_commit))),
            ("shots_in", text(&String::from_utf8_lossy(&self.shots_in))),
            ("shots_out", text(&String::from_utf8_lossy(&self.shots_out))),
            ("out_log", log),
            ("owe", text(&owe)),
            ("owe_ship", JsonValue::Number(self.owe_ship.into())),
            ("my_sunk", bytes(&self.my_sunk)),
            ("their_sunk", bytes(&self.their_sunk)),
            ("owes_reveal", JsonValue::Bool(self.owes_reveal)),
            ("await_reveal", JsonValue::Bool(self.await_reveal)),
            ("their_board", text(&self.their_board)),
            ("winner", text(&self.winner)),
            ("terminal", text(&self.terminal)),
            ("cheat_reason", text(&self.cheat_reason)),
            ("last_in", cell(self.last_in)),
            ("last_out", cell(self.last_out)),
        ];
        for (key, value) in pairs {
            session.metadata.insert(key.into(), value);
        }
    }

    fn hits_out(&self) -> usize {
        self.shots_out.iter().filter(|b| **b == b'h').count()
    }

    fn sunk_cells(&self) -> usize {
        self.their_sunk
            .iter()
            .map(|ship| SHIP_LENGTHS[usize::from(*ship)])
            .sum()
    }

    fn maybe_enter_battle(&mut self) {
        if self.phase == PHASE_SETUP && !self.my_commit.is_empty() && !self.their_commit.is_empty()
        {
            self.phase = PHASE_BATTLE.into();
            self.turn = self.first_turn.clone();
        }
    }
}

// ---------------------------------------------------------------------------
// Game rules
// ---------------------------------------------------------------------------

/// Why an answer from the opponent was refused.
enum AnswerFault {
    /// Malformed or out of sequence: reject the message, change nothing.
    Invalid(String),
    /// Provably impossible for any legal fleet: the sender is cheating.
    Cheat(String),
}

/// Apply the opponent's answer to my pending shot.
fn process_answer(game: &mut Game, answer: &str, ship: Option<u64>) -> Result<(), AnswerFault> {
    let invalid = |message: &str| AnswerFault::Invalid(message.into());
    let pending: Vec<usize> = (0..CELLS).filter(|c| game.shots_out[*c] == b'p').collect();
    let [cell] = pending[..] else {
        return Err(invalid("no shot is awaiting an answer"));
    };
    let (mark, kind, ship_index) = match (answer, ship) {
        ("m", None) => (b'm', b'm', -1),
        ("h", None) => (b'h', b'h', -1),
        ("s", Some(ship)) if ship < SHIP_LENGTHS.len() as u64 => {
            if game.their_sunk.contains(&(ship as u8)) {
                return Err(invalid("that ship was already reported sunk"));
            }
            (b'h', b's', ship as i64)
        }
        _ => return Err(invalid("answer is not valid")),
    };
    game.shots_out[cell] = mark;
    game.out_log.push(LogEntry {
        cell,
        answer: kind,
        ship: ship_index,
    });
    if kind == b's' {
        game.their_sunk.push(ship_index as u8);
    }

    let hits = game.hits_out();
    let unanswered = game.shots_out.iter().filter(|b| **b == b'_').count();
    let cheat = |reason: &str| Err(AnswerFault::Cheat(reason.into()));
    if hits > SHIP_CELLS {
        return cheat("reported more than 17 hits");
    }
    if game.sunk_cells() > hits {
        return cheat("reported a ship sunk without enough hits");
    }
    if hits == SHIP_CELLS && game.their_sunk.len() < SHIP_LENGTHS.len() {
        return cheat("every ship cell was hit but not every ship was reported sunk");
    }
    if unanswered == 0 && hits != SHIP_CELLS {
        return cheat("every cell was answered but the hits do not match a legal fleet");
    }
    Ok(())
}

/// Record the opponent's shot on my board and compute my answer.
fn receive_shot(game: &mut Game, cell: usize) -> Result<(), String> {
    let layout =
        layout_from_fleet(&game.my_fleet).ok_or_else(|| "own fleet is missing".to_string())?;
    if game.shots_in[cell] != b'_' {
        return Err("that cell was already fired at".into());
    }
    let id = layout[cell];
    if id == WATER {
        game.shots_in[cell] = b'm';
        game.owe = b'm';
        game.owe_ship = -1;
    } else {
        game.shots_in[cell] = b'h';
        let all_hit = (0..CELLS)
            .filter(|c| layout[*c] == id)
            .all(|c| game.shots_in[c] == b'h');
        if all_hit {
            game.owe = b's';
            game.owe_ship = i64::from(id);
            game.my_sunk.push(id);
        } else {
            game.owe = b'h';
            game.owe_ship = -1;
        }
    }
    game.last_in = Some(cell);
    Ok(())
}

/// Check an opponent reveal against its commitment and every answer I
/// received. Returns the revealed board string.
fn verify_reveal(
    game: &Game,
    session_id: &str,
    player: &str,
    fleet: &[u8],
    salt: &[u8],
) -> Result<String, String> {
    if salt.len() != SALT_BYTES {
        return Err("revealed salt has the wrong length".into());
    }
    if commitment(session_id, player, fleet, salt).as_slice() != game.their_commit.as_slice() {
        return Err("the revealed fleet does not match the commitment".into());
    }
    let layout =
        layout_from_fleet(fleet).ok_or_else(|| "the revealed fleet is illegal".to_string())?;
    let mut hits = [0usize; 5];
    for (index, entry) in game.out_log.iter().enumerate() {
        let id = layout[entry.cell];
        let (expected, ship) = if id == WATER {
            (b'm', -1)
        } else {
            hits[usize::from(id)] += 1;
            if hits[usize::from(id)] == SHIP_LENGTHS[usize::from(id)] {
                (b's', i64::from(id))
            } else {
                (b'h', -1)
            }
        };
        if entry.answer != expected || entry.ship != ship {
            return Err(format!(
                "answer {} contradicts the revealed fleet",
                index + 1
            ));
        }
    }
    Ok(board_string(&layout))
}

/// End the game. `terminal` is `win`, `resign`, `cheat`, or `void`.
fn finish(session: &mut Session, game: &mut Game, winner: &str, terminal: &str, reason: &str) {
    game.phase = PHASE_DONE.into();
    game.turn.clear();
    game.owes_reveal = false;
    game.await_reveal = false;
    game.winner = winner.into();
    game.terminal = terminal.into();
    game.cheat_reason = reason.into();
    let _ = SessionStateMachine::apply_command(session, CMD_MOVE, true);
}

// ---------------------------------------------------------------------------
// The app
// ---------------------------------------------------------------------------

/// The Battleship LRGP application.
pub struct BattleshipApp {
    sessions: Mutex<HashMap<(String, String), Session>>,
}

impl Default for BattleshipApp {
    fn default() -> Self {
        Self::new()
    }
}

impl BattleshipApp {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
        }
    }

    fn ttl_policy() -> HashMap<String, f64> {
        HashMap::from([
            (STATUS_PENDING.into(), TTL_PENDING),
            (STATUS_ACTIVE.into(), TTL_ACTIVE),
        ])
    }

    fn get_session(&self, session_id: &str, identity_id: &str) -> Option<Session> {
        let mut sessions = self.sessions.lock().unwrap();
        let session = sessions.get_mut(&(session_id.into(), identity_id.into()))?;
        SessionStateMachine::check_expiry(session, Some(&Self::ttl_policy()), None);
        Some(session.clone())
    }

    fn save_session(&self, session: &Session) {
        self.sessions.lock().unwrap().insert(
            (session.session_id.clone(), session.identity_id.clone()),
            session.clone(),
        );
    }

    fn new_session(session_id: &str, identity: &str, contact: &str, initiator: &str) -> Session {
        let mut session = Session::new(session_id);
        session.identity_id = identity.into();
        session.app_id = APP_ID.into();
        session.app_version = APP_VERSION;
        session.contact_hash = contact.into();
        session.initiator = initiator.into();
        Game::new(initiator).store(&mut session);
        session
    }

    // -- wire validation ----------------------------------------------------

    fn validate_wire_payload(
        command: &str,
        payload: &HashMap<String, rmpv::Value>,
    ) -> Result<(), String> {
        match command {
            CMD_CHALLENGE | CMD_ACCEPT | CMD_DECLINE | CMD_RESIGN => {
                if !payload.is_empty() {
                    return Err(format!("{command} payload must be empty"));
                }
            }
            CMD_MOVE => Self::validate_move_wire(payload)?,
            CMD_ERROR => {}
            _ => return Err(format!("unsupported command '{command}'")),
        }
        Ok(())
    }

    fn validate_move_wire(payload: &HashMap<String, rmpv::Value>) -> Result<(), String> {
        let kind = payload
            .get("k")
            .and_then(value_as_str)
            .ok_or_else(|| "move k must be a string".to_string())?;
        match kind {
            "c" => {
                if !exact_keys(payload, &["k", "h"]) {
                    return Err("commit payload has invalid keys".into());
                }
                let valid = payload
                    .get("h")
                    .and_then(value_as_bytes)
                    .is_some_and(|h| h.len() == COMMIT_BYTES);
                if !valid {
                    return Err("commit h must be 32 bytes".into());
                }
            }
            "f" => {
                let number = payload
                    .get("n")
                    .and_then(value_as_u64)
                    .ok_or_else(|| "fire n must be an integer".to_string())?;
                if payload
                    .get("c")
                    .and_then(value_as_u64)
                    .is_none_or(|cell| cell >= CELLS as u64)
                {
                    return Err("fire c must be a cell index".into());
                }
                if number == 1 {
                    if !exact_keys(payload, &["k", "n", "c", "h"]) {
                        return Err("first fire payload has invalid keys".into());
                    }
                    let valid = payload
                        .get("h")
                        .and_then(value_as_bytes)
                        .is_some_and(|h| h.len() == COMMIT_BYTES);
                    if !valid {
                        return Err("first fire h must be 32 bytes".into());
                    }
                } else {
                    let answer = payload
                        .get("a")
                        .and_then(value_as_str)
                        .ok_or_else(|| "fire a must be a string".to_string())?;
                    match answer {
                        "m" | "h" => {
                            if !exact_keys(payload, &["k", "n", "c", "a"]) {
                                return Err("fire payload has invalid keys".into());
                            }
                        }
                        "s" => {
                            if !exact_keys(payload, &["k", "n", "c", "a", "s"]) {
                                return Err("sunk fire payload has invalid keys".into());
                            }
                            if payload
                                .get("s")
                                .and_then(value_as_u64)
                                .is_none_or(|ship| ship >= SHIP_LENGTHS.len() as u64)
                            {
                                return Err("fire s must be a ship index".into());
                            }
                        }
                        _ => return Err("fire a is not recognized".into()),
                    }
                }
            }
            "r" => {
                let has_answer = payload.contains_key("a");
                let keys: &[&str] = if has_answer {
                    &["k", "n", "a", "s", "b", "z"]
                } else {
                    &["k", "n", "b", "z"]
                };
                if !exact_keys(payload, keys) {
                    return Err("reveal payload has invalid keys".into());
                }
                if payload.get("n").and_then(value_as_u64).is_none() {
                    return Err("reveal n must be an integer".into());
                }
                let fleet_ok = payload
                    .get("b")
                    .and_then(value_as_bytes)
                    .is_some_and(|b| b.len() == SHIP_LENGTHS.len());
                let salt_ok = payload
                    .get("z")
                    .and_then(value_as_bytes)
                    .is_some_and(|z| z.len() == SALT_BYTES);
                if !fleet_ok || !salt_ok {
                    return Err("reveal b/z have the wrong size".into());
                }
                if has_answer {
                    let answer_ok = payload.get("a").and_then(value_as_str) == Some("s");
                    let ship_ok = payload
                        .get("s")
                        .and_then(value_as_u64)
                        .is_some_and(|ship| ship < SHIP_LENGTHS.len() as u64);
                    if !answer_ok || !ship_ok {
                        return Err("reveal answer must be a sunk ship".into());
                    }
                }
            }
            _ => return Err("move kind is not recognized".into()),
        }
        Ok(())
    }

    // -- incoming moves -----------------------------------------------------

    fn apply_move_in(
        session: &mut Session,
        payload: &HashMap<String, rmpv::Value>,
        sender: &str,
    ) -> Result<(), String> {
        if session.status != STATUS_ACTIVE {
            return Err(format!("Session is not active ({})", session.status));
        }
        let opponent = other_player(session, &session.identity_id.clone())
            .ok_or_else(|| "Opponent unknown".to_string())?;
        if sender != opponent {
            return Err("Sender is not the opponent".into());
        }
        let mut game = Game::load(session)?;
        let kind = payload.get("k").and_then(value_as_str).unwrap_or("");
        match kind {
            "c" => Self::commit_in(session, &mut game, payload)?,
            "f" => Self::fire_in(session, &mut game, payload, sender)?,
            "r" => Self::reveal_in(session, &mut game, payload, sender)?,
            _ => return Err("move kind is not recognized".into()),
        }
        game.store(session);
        Ok(())
    }

    fn commit_in(
        session: &mut Session,
        game: &mut Game,
        payload: &HashMap<String, rmpv::Value>,
    ) -> Result<(), String> {
        let commitment = payload
            .get("h")
            .and_then(value_as_bytes)
            .unwrap_or_default();
        if !game.their_commit.is_empty() {
            // A first-shot copy of the commitment may have beaten this message.
            return if game.their_commit == commitment {
                Ok(())
            } else {
                Err("a different commitment is already recorded".into())
            };
        }
        if game.phase != PHASE_SETUP {
            return Err("commitments are only accepted during setup".into());
        }
        game.their_commit = commitment.to_vec();
        game.maybe_enter_battle();
        let _ = SessionStateMachine::apply_command(session, CMD_MOVE, false);
        Ok(())
    }

    fn fire_in(
        session: &mut Session,
        game: &mut Game,
        payload: &HashMap<String, rmpv::Value>,
        sender: &str,
    ) -> Result<(), String> {
        let number = payload.get("n").and_then(value_as_u64).unwrap_or(0);
        let cell = payload.get("c").and_then(value_as_u64).unwrap_or(0) as usize;

        if number == 1 {
            // The first shot repeats the shooter's commitment so a reordered
            // commit message cannot strand the game.
            let commitment = payload
                .get("h")
                .and_then(value_as_bytes)
                .unwrap_or_default();
            if game.my_commit.is_empty() {
                return Err("Place your fleet before the battle starts".into());
            }
            if game.their_commit.is_empty() {
                game.their_commit = commitment.to_vec();
            } else if game.their_commit != commitment {
                return Err("a different commitment is already recorded".into());
            }
            game.maybe_enter_battle();
        }
        if game.phase != PHASE_BATTLE {
            return Err("The battle has not started".into());
        }
        if game.turn != sender {
            return Err("Not your turn".into());
        }
        if number != game.move_count + 1 {
            return Err(format!(
                "Move number mismatch: expected {}, got {number}",
                game.move_count + 1
            ));
        }
        if game.shots_in[cell] != b'_' {
            return Err("That cell was already fired at".into());
        }

        if number > 1 {
            let answer = payload.get("a").and_then(value_as_str).unwrap_or("");
            let ship = payload.get("s").and_then(value_as_u64);
            match process_answer(game, answer, ship) {
                Ok(()) => {}
                Err(AnswerFault::Invalid(message)) => return Err(message),
                Err(AnswerFault::Cheat(reason)) => {
                    game.move_count = number;
                    let me = session.identity_id.clone();
                    finish(session, game, &me, "cheat", &reason);
                    return Ok(());
                }
            }
            if game.their_sunk.len() == SHIP_LENGTHS.len() {
                return Err("All ships were reported sunk: a reveal was expected".into());
            }
        }

        receive_shot(game, cell)?;
        game.move_count = number;
        if game.my_sunk.len() == SHIP_LENGTHS.len() {
            // Defeat: the final answer travels inside my reveal.
            game.phase = PHASE_REVEAL.into();
            game.turn.clear();
            game.owes_reveal = true;
        } else {
            game.turn = session.identity_id.clone();
        }
        let _ = SessionStateMachine::apply_command(session, CMD_MOVE, false);
        Ok(())
    }

    fn reveal_in(
        session: &mut Session,
        game: &mut Game,
        payload: &HashMap<String, rmpv::Value>,
        sender: &str,
    ) -> Result<(), String> {
        let number = payload.get("n").and_then(value_as_u64).unwrap_or(0);
        let fleet = payload
            .get("b")
            .and_then(value_as_bytes)
            .unwrap_or_default();
        let salt = payload
            .get("z")
            .and_then(value_as_bytes)
            .unwrap_or_default();
        if number != game.move_count + 1 {
            return Err(format!(
                "Move number mismatch: expected {}, got {number}",
                game.move_count + 1
            ));
        }
        let me = session.identity_id.clone();

        if payload.contains_key("a") {
            // The opponent lost: final answer + reveal.
            if game.phase != PHASE_BATTLE || game.turn != sender {
                return Err("A reveal was not expected".into());
            }
            let ship = payload.get("s").and_then(value_as_u64);
            match process_answer(game, "s", ship) {
                Ok(()) => {}
                Err(AnswerFault::Invalid(message)) => return Err(message),
                Err(AnswerFault::Cheat(reason)) => {
                    game.move_count = number;
                    finish(session, game, &me, "cheat", &reason);
                    return Ok(());
                }
            }
            game.move_count = number;
            if game.their_sunk.len() != SHIP_LENGTHS.len() {
                finish(
                    session,
                    game,
                    &me,
                    "cheat",
                    "claimed defeat before every ship was sunk",
                );
                return Ok(());
            }
            match verify_reveal(game, &session.session_id, sender, fleet, salt) {
                Ok(board) => {
                    game.their_board = board;
                    game.phase = PHASE_REVEAL.into();
                    game.turn.clear();
                    game.owes_reveal = true;
                    let _ = SessionStateMachine::apply_command(session, CMD_MOVE, false);
                }
                Err(reason) => finish(session, game, &me, "cheat", &reason),
            }
            return Ok(());
        }

        // The winner's reveal, answering mine.
        if game.phase != PHASE_REVEAL || !game.await_reveal {
            return Err("A reveal was not expected".into());
        }
        game.move_count = number;
        match verify_reveal(game, &session.session_id, sender, fleet, salt) {
            Ok(board) => {
                game.their_board = board;
                finish(session, game, sender, "win", "");
            }
            Err(reason) => finish(session, game, &me, "cheat", &reason),
        }
        Ok(())
    }

    fn handle_move_in(
        &self,
        session_id: &str,
        payload: &HashMap<String, rmpv::Value>,
        sender: &str,
        identity: &str,
    ) -> IncomingResult {
        let Some(session) = self.get_session(session_id, identity) else {
            return error_result(ERR_PROTOCOL_ERROR, "Unknown session");
        };
        let mut working = session;
        if let Err(message) = Self::apply_move_in(&mut working, payload, sender) {
            return error_result(ERR_INVALID_MOVE, &message);
        }
        working.unread = 1;
        self.save_session(&working);
        let mut emit = emit_event("move", session_id, sender);
        emit.insert(
            "payload".into(),
            JsonValue::Object(
                payload
                    .iter()
                    // The commitment/fleet bytes are not UI-relevant.
                    .filter(|(key, _)| !matches!(key.as_str(), "h" | "b" | "z"))
                    .map(|(key, value)| (key.clone(), rmpv_to_json(value)))
                    .collect(),
            ),
        );
        IncomingResult {
            session: Some(session_to_json(&working)),
            emit: Some(emit),
            error: None,
        }
    }

    // -- challenge / accept / decline / resign ------------------------------

    fn handle_challenge_in(
        &self,
        session_id: &str,
        sender: &str,
        identity: &str,
    ) -> IncomingResult {
        if let Some(existing) = self.get_session(session_id, identity) {
            return IncomingResult {
                session: Some(session_to_json(&existing)),
                emit: None,
                error: None,
            };
        }
        let mut session = Self::new_session(session_id, identity, sender, sender);
        session.unread = 1;
        self.save_session(&session);
        IncomingResult {
            session: Some(session_to_json(&session)),
            emit: Some(emit_event("challenge", session_id, sender)),
            error: None,
        }
    }

    fn handle_accept_in(&self, session_id: &str, sender: &str, identity: &str) -> IncomingResult {
        let Some(mut session) = self.get_session(session_id, identity) else {
            return error_result(ERR_PROTOCOL_ERROR, "Unknown session");
        };
        if session.contact_hash.is_empty() {
            session.contact_hash = sender.into();
        }
        if let Err(error) = SessionStateMachine::apply_command(&mut session, CMD_ACCEPT, false) {
            return error_result(ERR_PROTOCOL_ERROR, &error.to_string());
        }
        session.unread = 1;
        self.save_session(&session);
        IncomingResult {
            session: Some(session_to_json(&session)),
            emit: Some(emit_event("accept", session_id, sender)),
            error: None,
        }
    }

    fn handle_end_in(
        &self,
        session_id: &str,
        command: &str,
        sender: &str,
        identity: &str,
    ) -> IncomingResult {
        let Some(mut session) = self.get_session(session_id, identity) else {
            return error_result(ERR_PROTOCOL_ERROR, "Unknown session");
        };
        if let Err(error) = SessionStateMachine::apply_command(&mut session, command, false) {
            return error_result(ERR_PROTOCOL_ERROR, &error.to_string());
        }
        if command == CMD_RESIGN {
            let Ok(mut game) = Game::load(&session) else {
                return error_result(ERR_PROTOCOL_ERROR, "Stored game is invalid");
            };
            game.phase = PHASE_DONE.into();
            game.turn.clear();
            game.owes_reveal = false;
            game.await_reveal = false;
            game.winner = identity.into();
            game.terminal = "resign".into();
            game.store(&mut session);
        }
        session.unread = 1;
        self.save_session(&session);
        IncomingResult {
            session: Some(session_to_json(&session)),
            emit: Some(emit_event(command, session_id, sender)),
            error: None,
        }
    }

    // -- outgoing -----------------------------------------------------------

    fn rejected(message: impl Into<String>) -> OutgoingResult {
        OutgoingResult {
            payload: HashMap::new(),
            fallback_text: format!("[LRGP Battleship] {}", message.into()),
        }
    }

    fn parse_fleet_intent(intent: &HashMap<String, rmpv::Value>) -> Result<Vec<u8>, String> {
        let list = match intent.get("p") {
            Some(rmpv::Value::Array(list)) => list,
            _ => return Err("commit intent p must be an array of five ships".into()),
        };
        let fleet: Vec<u8> = list
            .iter()
            .map(|value| value_as_u64(value).and_then(|n| u8::try_from(n).ok()))
            .collect::<Option<Vec<u8>>>()
            .ok_or_else(|| "commit intent p must contain bytes".to_string())?;
        if layout_from_fleet(&fleet).is_none() {
            return Err("That fleet layout is not legal".into());
        }
        Ok(fleet)
    }

    fn validate_move_intent(
        session: &Session,
        game: &Game,
        intent: &HashMap<String, rmpv::Value>,
        identity: &str,
    ) -> Result<(), String> {
        if session.status != STATUS_ACTIVE {
            return Err(format!("Session is not active ({})", session.status));
        }
        if session.contact_hash.is_empty() {
            return Err("Opponent unknown".into());
        }
        let kind = intent
            .get("k")
            .and_then(value_as_str)
            .ok_or_else(|| "move intent k must be a string".to_string())?;
        match kind {
            "c" => {
                if !exact_keys(intent, &["k", "p"]) {
                    return Err("commit intent must contain exactly k and p".into());
                }
                if game.phase != PHASE_SETUP || !game.my_commit.is_empty() {
                    return Err("Your fleet is already placed".into());
                }
                Self::parse_fleet_intent(intent).map(|_| ())
            }
            "f" => {
                if !exact_keys(intent, &["k", "c"]) {
                    return Err("fire intent must contain exactly k and c".into());
                }
                let cell = intent
                    .get("c")
                    .and_then(value_as_u64)
                    .filter(|cell| *cell < CELLS as u64)
                    .ok_or_else(|| "Invalid cell".to_string())?;
                if game.phase != PHASE_BATTLE {
                    return Err("The battle has not started".into());
                }
                if game.turn != identity {
                    return Err("Not your turn".into());
                }
                if game.shots_out[cell as usize] != b'_' {
                    return Err("You already fired at that cell".into());
                }
                Ok(())
            }
            "r" => {
                if !exact_keys(intent, &["k"]) {
                    return Err("reveal intent must contain exactly k".into());
                }
                if game.phase != PHASE_REVEAL || !game.owes_reveal {
                    return Err("There is nothing to reveal yet".into());
                }
                Ok(())
            }
            _ => Err("move kind is not recognized".into()),
        }
    }

    fn handle_move_out(
        &self,
        session_id: &str,
        intent: &HashMap<String, rmpv::Value>,
        identity: &str,
    ) -> OutgoingResult {
        let Some(mut session) = self.get_session(session_id, identity) else {
            return Self::rejected("Session not found");
        };
        let Ok(mut game) = Game::load(&session) else {
            return Self::rejected("Stored game is invalid");
        };
        if let Err(message) = Self::validate_move_intent(&session, &game, intent, identity) {
            return Self::rejected(message);
        }
        let kind = intent.get("k").and_then(value_as_str).unwrap_or("");
        let (payload, fallback) = match kind {
            "c" => {
                let fleet = match Self::parse_fleet_intent(intent) {
                    Ok(fleet) => fleet,
                    Err(message) => return Self::rejected(message),
                };
                let salt = random_salt();
                let digest = commitment(session_id, identity, &fleet, &salt);
                game.my_fleet = fleet;
                game.my_salt = salt.to_vec();
                game.my_commit = digest.to_vec();
                game.maybe_enter_battle();
                (
                    HashMap::from([
                        ("k".into(), rmpv::Value::String("c".into())),
                        ("h".into(), rmpv::Value::Binary(digest.to_vec())),
                    ]),
                    "[LRGP Battleship] Fleet ready".to_string(),
                )
            }
            "f" => {
                let cell = intent.get("c").and_then(value_as_u64).unwrap_or(0) as usize;
                let number = game.move_count + 1;
                let mut payload = HashMap::from([
                    ("k".into(), rmpv::Value::String("f".into())),
                    ("n".into(), rmpv::Value::Integer(number.into())),
                    ("c".into(), rmpv::Value::Integer((cell as u64).into())),
                ]);
                if number == 1 {
                    payload.insert("h".into(), rmpv::Value::Binary(game.my_commit.clone()));
                } else {
                    let answer = if game.owe == 0 { b'm' } else { game.owe };
                    payload.insert(
                        "a".into(),
                        rmpv::Value::String(char::from(answer).to_string().into()),
                    );
                    if answer == b's' {
                        payload.insert(
                            "s".into(),
                            rmpv::Value::Integer((game.owe_ship.max(0) as u64).into()),
                        );
                    }
                }
                game.shots_out[cell] = b'p';
                game.last_out = Some(cell);
                game.owe = 0;
                game.owe_ship = -1;
                game.move_count = number;
                game.turn = session.contact_hash.clone();
                (
                    payload,
                    format!("[LRGP Battleship] Fire at {}", cell_label(cell)),
                )
            }
            _ => {
                // Reveal: the loser's carries the final answer.
                let number = game.move_count + 1;
                let lost = game.my_sunk.len() == SHIP_LENGTHS.len();
                let mut payload = HashMap::from([
                    ("k".into(), rmpv::Value::String("r".into())),
                    ("n".into(), rmpv::Value::Integer(number.into())),
                    ("b".into(), rmpv::Value::Binary(game.my_fleet.clone())),
                    ("z".into(), rmpv::Value::Binary(game.my_salt.clone())),
                ]);
                game.move_count = number;
                game.owes_reveal = false;
                if lost {
                    payload.insert("a".into(), rmpv::Value::String("s".into()));
                    payload.insert(
                        "s".into(),
                        rmpv::Value::Integer((game.owe_ship.max(0) as u64).into()),
                    );
                    game.owe = 0;
                    game.owe_ship = -1;
                    game.await_reveal = true;
                    let _ = SessionStateMachine::apply_command(&mut session, CMD_MOVE, false);
                } else {
                    let me = identity.to_string();
                    finish(&mut session, &mut game, &me, "win", "");
                }
                (payload, "[LRGP Battleship] Fleet revealed".to_string())
            }
        };
        if kind != "r" {
            let _ = SessionStateMachine::apply_command(&mut session, CMD_MOVE, false);
        }
        game.store(&mut session);
        self.save_session(&session);
        OutgoingResult {
            payload,
            fallback_text: fallback,
        }
    }

    fn handle_end_out(&self, session_id: &str, command: &str, identity: &str) -> OutgoingResult {
        let Some(mut session) = self.get_session(session_id, identity) else {
            return Self::rejected("Session not found");
        };
        let _ = SessionStateMachine::apply_command(&mut session, command, false);
        if command == CMD_RESIGN
            && let Ok(mut game) = Game::load(&session)
        {
            game.phase = PHASE_DONE.into();
            game.turn.clear();
            game.owes_reveal = false;
            game.await_reveal = false;
            game.winner = session.contact_hash.clone();
            game.terminal = "resign".into();
            game.store(&mut session);
        }
        self.save_session(&session);
        OutgoingResult {
            payload: HashMap::new(),
            fallback_text: Self::fallback(command, &HashMap::new()),
        }
    }

    fn fallback(command: &str, payload: &HashMap<String, rmpv::Value>) -> String {
        match command {
            CMD_CHALLENGE => "[LRGP Battleship] Sent a challenge!".into(),
            CMD_ACCEPT => "[LRGP Battleship] Challenge accepted".into(),
            CMD_DECLINE => "[LRGP Battleship] Challenge declined".into(),
            CMD_RESIGN => "[LRGP Battleship] Resigned.".into(),
            CMD_MOVE => match payload.get("k").and_then(value_as_str) {
                Some("c") => "[LRGP Battleship] Fleet ready".into(),
                Some("f") => format!(
                    "[LRGP Battleship] Fire at {}",
                    cell_label(
                        payload.get("c").and_then(value_as_u64).unwrap_or(0) as usize % CELLS
                    )
                ),
                Some("r") => "[LRGP Battleship] Fleet revealed".into(),
                _ => "[LRGP Battleship] Move".into(),
            },
            other => format!("[LRGP Battleship] {other}"),
        }
    }

    // -- restore validation -------------------------------------------------

    fn validate_restored_session(session: &Session) -> Result<(), LrgpError> {
        let invalid = |message: &str| LrgpError::Validation {
            code: ERR_PROTOCOL_ERROR.into(),
            message: message.into(),
        };
        let game = Game::load(session).map_err(|message| invalid(&message))?;
        if game.first_turn != session.initiator {
            return Err(invalid("restored first_turn must equal the challenger"));
        }
        if !game.my_fleet.is_empty() {
            if game.my_salt.is_empty() || game.my_commit.is_empty() {
                return Err(invalid("restored fleet requires a salt and a commitment"));
            }
            if layout_from_fleet(&game.my_fleet).is_none() {
                return Err(invalid("restored fleet is illegal"));
            }
            let expected = commitment(
                &session.session_id,
                &session.identity_id,
                &game.my_fleet,
                &game.my_salt,
            );
            if expected.as_slice() != game.my_commit.as_slice() {
                return Err(invalid("restored commitment does not match the fleet"));
            }
        } else if !game.my_commit.is_empty() || !game.my_salt.is_empty() {
            return Err(invalid("restored commitment has no fleet"));
        }
        let answered = game
            .shots_out
            .iter()
            .filter(|b| matches!(**b, b'm' | b'h'))
            .count();
        if answered != game.out_log.len() {
            return Err(invalid("restored answer log does not match the shot grid"));
        }
        if game.shots_out.iter().filter(|b| **b == b'p').count() > 1 {
            return Err(invalid("restored game has several unanswered shots"));
        }
        if game
            .my_sunk
            .iter()
            .chain(game.their_sunk.iter())
            .any(|s| usize::from(*s) >= SHIP_LENGTHS.len())
        {
            return Err(invalid("restored sunk list contains an unknown ship"));
        }
        match session.status.as_str() {
            STATUS_PENDING | STATUS_DECLINED => {
                if game.phase != PHASE_SETUP
                    || game.move_count != 0
                    || !game.my_commit.is_empty()
                    || !game.their_commit.is_empty()
                {
                    return Err(invalid(
                        "pending or declined session contains game progress",
                    ));
                }
            }
            STATUS_ACTIVE => {
                if game.phase == PHASE_DONE {
                    return Err(invalid("active session cannot be done"));
                }
                if game.phase == PHASE_BATTLE
                    && (game.my_commit.is_empty()
                        || game.their_commit.is_empty()
                        || (game.turn != session.identity_id && game.turn != session.contact_hash))
                {
                    return Err(invalid("battle phase requires both commitments and a turn"));
                }
            }
            STATUS_COMPLETED => {
                if game.phase != PHASE_DONE || game.terminal.is_empty() {
                    return Err(invalid(
                        "completed session must be done with a terminal reason",
                    ));
                }
            }
            STATUS_EXPIRED => {}
            _ => return Err(invalid("restored status is not recognized")),
        }
        Ok(())
    }
}

impl GameApp for BattleshipApp {
    fn app_id(&self) -> &str {
        APP_ID
    }

    fn version(&self) -> u32 {
        APP_VERSION
    }

    fn manifest(&self) -> AppManifest {
        let actions: Vec<String> = vec![
            CMD_CHALLENGE.into(),
            CMD_ACCEPT.into(),
            CMD_DECLINE.into(),
            CMD_MOVE.into(),
            CMD_RESIGN.into(),
            CMD_ERROR.into(),
        ];
        let mut preferred_delivery = HashMap::new();
        for command in &actions {
            preferred_delivery.insert(command.clone(), "opportunistic".into());
        }
        preferred_delivery.insert(CMD_RESIGN.into(), "direct".into());
        AppManifest {
            app_id: APP_ID.into(),
            version: APP_VERSION,
            display_name: "Battleship".into(),
            icon: APP_ID.into(),
            session_type: SESSION_TURN_BASED.into(),
            max_players: 2,
            validation: VALIDATION_BOTH.into(),
            actions,
            preferred_delivery,
            ttl: Self::ttl_policy(),
        }
    }

    fn handle_incoming(
        &self,
        session_id: &str,
        command: &str,
        payload: &HashMap<String, rmpv::Value>,
        sender_hash: &str,
        identity_id: &str,
    ) -> IncomingResult {
        if command != CMD_ERROR
            && let Err(message) = Self::validate_wire_payload(command, payload)
        {
            return error_result(ERR_PROTOCOL_ERROR, &message);
        }
        match command {
            CMD_CHALLENGE => self.handle_challenge_in(session_id, sender_hash, identity_id),
            CMD_ACCEPT => self.handle_accept_in(session_id, sender_hash, identity_id),
            CMD_MOVE => self.handle_move_in(session_id, payload, sender_hash, identity_id),
            CMD_DECLINE | CMD_RESIGN => {
                self.handle_end_in(session_id, command, sender_hash, identity_id)
            }
            CMD_ERROR => IncomingResult {
                session: None,
                emit: None,
                error: Some(
                    payload
                        .iter()
                        .map(|(key, value)| (key.clone(), rmpv_to_json(value)))
                        .collect(),
                ),
            },
            _ => error_result(ERR_PROTOCOL_ERROR, "Unknown command"),
        }
    }

    fn handle_outgoing(
        &self,
        session_id: &str,
        command: &str,
        payload: &HashMap<String, rmpv::Value>,
        identity_id: &str,
    ) -> OutgoingResult {
        match command {
            CMD_CHALLENGE => {
                let sid = if session_id.is_empty() {
                    gen_session_id()
                } else {
                    session_id.into()
                };
                let session = Self::new_session(&sid, identity_id, "", identity_id);
                self.save_session(&session);
                OutgoingResult {
                    payload: HashMap::new(),
                    fallback_text: Self::fallback(command, payload),
                }
            }
            CMD_ACCEPT => {
                let Some(mut session) = self.get_session(session_id, identity_id) else {
                    return Self::rejected("Session not found");
                };
                let _ = SessionStateMachine::apply_command(&mut session, CMD_ACCEPT, false);
                self.save_session(&session);
                OutgoingResult {
                    payload: HashMap::new(),
                    fallback_text: Self::fallback(command, payload),
                }
            }
            CMD_MOVE => self.handle_move_out(session_id, payload, identity_id),
            CMD_DECLINE | CMD_RESIGN => self.handle_end_out(session_id, command, identity_id),
            _ => OutgoingResult {
                payload: payload.clone(),
                fallback_text: Self::fallback(command, payload),
            },
        }
    }

    fn validate_action(
        &self,
        session_id: &str,
        command: &str,
        payload: &HashMap<String, rmpv::Value>,
        identity_id: &str,
    ) -> (bool, Option<String>) {
        if let Err(message) = Self::validate_wire_payload(command, payload) {
            return (false, Some(message));
        }
        if self.get_session(session_id, identity_id).is_none() && command != CMD_CHALLENGE {
            return (false, Some("Session not found".into()));
        }
        (true, None)
    }

    fn validate_outgoing_action(
        &self,
        session_id: &str,
        command: &str,
        payload: &HashMap<String, rmpv::Value>,
        identity_id: &str,
    ) -> (bool, Option<String>) {
        match command {
            CMD_CHALLENGE | CMD_ACCEPT | CMD_DECLINE | CMD_RESIGN => {
                if !payload.is_empty() {
                    return (false, Some(format!("{command} payload must be empty")));
                }
            }
            CMD_MOVE => {}
            _ => return (false, Some(format!("Unsupported action: {command}"))),
        }
        let Some(session) = self.get_session(session_id, identity_id) else {
            return if command == CMD_CHALLENGE {
                (true, None)
            } else {
                (false, Some("Session not found".into()))
            };
        };
        if session.status == STATUS_EXPIRED {
            return (false, Some("Session expired".into()));
        }
        if command == CMD_CHALLENGE {
            return (false, Some("Session already exists".into()));
        }
        if command == CMD_MOVE {
            let game = match Game::load(&session) {
                Ok(game) => game,
                Err(_) => return (false, Some("Stored game is invalid".into())),
            };
            return match Self::validate_move_intent(&session, &game, payload, identity_id) {
                Ok(()) => (true, None),
                Err(message) => (false, Some(message)),
            };
        }
        let mut candidate = session;
        match SessionStateMachine::apply_command(&mut candidate, command, false) {
            Ok(_) => (true, None),
            Err(error) => (false, Some(error.to_string())),
        }
    }

    fn get_session_state(&self, session_id: &str, identity_id: &str) -> HashMap<String, JsonValue> {
        self.get_session(session_id, identity_id)
            .map(|session| session_to_json(&session))
            .unwrap_or_default()
    }

    fn render_fallback(&self, command: &str, payload: &HashMap<String, rmpv::Value>) -> String {
        Self::fallback(command, payload)
    }

    fn get_delivery_method(&self, command: &str) -> String {
        match command {
            CMD_RESIGN => "direct".into(),
            _ => "opportunistic".into(),
        }
    }

    fn get_session_record(&self, session_id: &str, identity_id: &str) -> Option<Session> {
        self.get_session(session_id, identity_id)
    }

    fn upsert_session(&self, mut session: Session) -> Result<(), LrgpError> {
        if session.app_id != APP_ID || session.app_version != APP_VERSION {
            return Err(LrgpError::Validation {
                code: ERR_UNSUPPORTED_APP.into(),
                message: "session app/version does not match Battleship".into(),
            });
        }
        if session.identity_id.is_empty() {
            return Err(LrgpError::Validation {
                code: ERR_PROTOCOL_ERROR.into(),
                message: "restored session must include identity_id".into(),
            });
        }
        if !lrgp::protocol::is_valid_session_id(&session.session_id) {
            return Err(LrgpError::InvalidEnvelope(
                "restored session id must be exactly 16 lowercase hexadecimal characters".into(),
            ));
        }
        Self::validate_restored_session(&session)?;
        SessionStateMachine::check_expiry(&mut session, Some(&Self::ttl_policy()), None);
        self.save_session(&session);
        Ok(())
    }

    fn remove_session(&self, session_id: &str, identity_id: &str) -> bool {
        self.sessions
            .lock()
            .unwrap()
            .remove(&(session_id.into(), identity_id.into()))
            .is_some()
    }

    fn list_session_records(&self, identity_id: Option<&str>) -> Vec<Session> {
        let mut sessions = self.sessions.lock().unwrap();
        let ttl = Self::ttl_policy();
        sessions
            .values_mut()
            .filter(|session| identity_id.is_none_or(|id| session.identity_id == id))
            .map(|session| {
                SessionStateMachine::check_expiry(session, Some(&ttl), None);
                session.clone()
            })
            .collect()
    }

    fn bind_session_peer(
        &self,
        session_id: &str,
        identity_id: &str,
        peer_hash: &str,
    ) -> Result<(), LrgpError> {
        if peer_hash.is_empty() {
            return Err(LrgpError::ParticipantRequired);
        }
        let mut sessions = self.sessions.lock().unwrap();
        let session = sessions
            .get_mut(&(session_id.into(), identity_id.into()))
            .ok_or_else(|| LrgpError::SessionNotFound(session_id.into()))?;
        if !session.contact_hash.is_empty() && session.contact_hash != peer_hash {
            return Err(LrgpError::UnauthorizedPeer {
                session_id: session_id.into(),
            });
        }
        session.contact_hash = peer_hash.into();
        Ok(())
    }

    fn authorize_incoming(
        &self,
        session_id: &str,
        command: &str,
        sender_hash: &str,
        identity_id: &str,
    ) -> Result<(), LrgpError> {
        let Some(session) = self.get_session(session_id, identity_id) else {
            return if command == CMD_CHALLENGE {
                Ok(())
            } else {
                Err(LrgpError::SessionNotFound(session_id.into()))
            };
        };
        if session.status == STATUS_EXPIRED {
            return Err(LrgpError::SessionExpired(session_id.into()));
        }
        if session.contact_hash.is_empty() || session.contact_hash != sender_hash {
            return Err(LrgpError::UnauthorizedPeer {
                session_id: session_id.into(),
            });
        }
        Ok(())
    }

    fn snapshot_session(&self, session_id: &str, identity_id: &str) -> Option<Session> {
        self.get_session(session_id, identity_id)
    }

    fn rollback_session(&self, session_id: &str, identity_id: &str, snapshot: Option<Session>) {
        match snapshot {
            Some(session) => self.save_session(&session),
            None => {
                self.sessions
                    .lock()
                    .unwrap()
                    .remove(&(session_id.into(), identity_id.into()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lrgp::app_base::IncomingDispatch;
    use lrgp::envelope::Envelope;

    const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    /// Horizontal ships on rows 0..4: cells 0-4, 10-13, 20-22, 30-32, 40-41.
    const FLEET_A: [u8; 5] = [0, 20, 40, 60, 80];
    /// Vertical ships on columns 9..5.
    const FLEET_B: [u8; 5] = [19, 17, 15, 13, 11];

    fn ship_cells(fleet: &[u8]) -> Vec<usize> {
        let layout = layout_from_fleet(fleet).unwrap();
        (0..CELLS).filter(|c| layout[*c] != WATER).collect()
    }

    fn water_cells(fleet: &[u8]) -> Vec<usize> {
        let layout = layout_from_fleet(fleet).unwrap();
        (0..CELLS).filter(|c| layout[*c] == WATER).collect()
    }

    struct Duel {
        a: LrgpRouter,
        b: LrgpRouter,
        sid: String,
        max_envelope: usize,
    }

    type Intent = HashMap<String, rmpv::Value>;

    fn fire(cell: usize) -> Intent {
        HashMap::from([
            ("k".into(), rmpv::Value::String("f".into())),
            ("c".into(), rmpv::Value::Integer((cell as u64).into())),
        ])
    }

    fn commit(fleet: &[u8]) -> Intent {
        HashMap::from([
            ("k".into(), rmpv::Value::String("c".into())),
            (
                "p".into(),
                rmpv::Value::Array(
                    fleet
                        .iter()
                        .map(|b| rmpv::Value::Integer(u64::from(*b).into()))
                        .collect(),
                ),
            ),
        ])
    }

    fn reveal() -> Intent {
        HashMap::from([("k".into(), rmpv::Value::String("r".into()))])
    }

    impl Duel {
        fn start() -> Self {
            let mut duel = Self {
                a: lrgp_router(),
                b: lrgp_router(),
                sid: String::new(),
                max_envelope: 0,
            };
            let prepared = duel
                .a
                .dispatch_outgoing_to(APP_ID, 1, CMD_CHALLENGE, "", &HashMap::new(), A, B)
                .unwrap();
            duel.sid = prepared.session_id.clone();
            duel.deliver(true, &prepared.envelope).unwrap();
            duel.send(false, CMD_ACCEPT, HashMap::new()).unwrap();
            duel
        }

        fn deliver(&mut self, from_a: bool, envelope: &Envelope) -> Result<(), String> {
            self.max_envelope = self
                .max_envelope
                .max(lrgp::protocol::pack_to_bytes(envelope).unwrap().len());
            let (router, sender, receiver) = if from_a {
                (&self.b, A, B)
            } else {
                (&self.a, B, A)
            };
            match router
                .dispatch_incoming(envelope, sender, receiver)
                .map_err(|e| e.to_string())?
            {
                IncomingDispatch::Applied(result) => match result.error {
                    None => Ok(()),
                    Some(error) => Err(error["msg"].as_str().unwrap_or("error").to_string()),
                },
                _ => Err("unexpected dispatch".into()),
            }
        }

        fn prepare(&self, from_a: bool, command: &str, intent: Intent) -> Result<Envelope, String> {
            let (router, me, peer) = if from_a {
                (&self.a, A, B)
            } else {
                (&self.b, B, A)
            };
            router
                .dispatch_outgoing_to(APP_ID, 1, command, &self.sid, &intent, me, peer)
                .map(|prepared| prepared.envelope)
                .map_err(|e| e.to_string())
        }

        fn send(&mut self, from_a: bool, command: &str, intent: Intent) -> Result<(), String> {
            let envelope = self.prepare(from_a, command, intent)?;
            self.deliver(from_a, &envelope)
        }

        fn session(&self, of_a: bool) -> Session {
            let (router, id) = if of_a { (&self.a, A) } else { (&self.b, B) };
            router
                .with_app(APP_ID, |app| app.get_session_record(&self.sid, id))
                .flatten()
                .unwrap()
        }

        fn game(&self, of_a: bool) -> Game {
            Game::load(&self.session(of_a)).unwrap()
        }

        fn ready(&mut self) {
            self.send(true, CMD_MOVE, commit(&FLEET_A)).unwrap();
            self.send(false, CMD_MOVE, commit(&FLEET_B)).unwrap();
        }
    }

    /// Alternate shots until A has sunk B's fleet; `tamper` may rewrite B's
    /// envelopes before delivery. Returns the sunk-reveal exchange result.
    fn play_until_a_wins(duel: &mut Duel, tamper: &dyn Fn(&mut Envelope)) {
        let a_targets = ship_cells(&FLEET_B).into_iter();
        let mut b_targets = water_cells(&FLEET_A).into_iter();
        for target in a_targets {
            duel.send(true, CMD_MOVE, fire(target)).unwrap();
            if duel.game(false).phase == PHASE_REVEAL {
                break;
            }
            let mut envelope = duel
                .prepare(false, CMD_MOVE, fire(b_targets.next().unwrap()))
                .unwrap();
            tamper(&mut envelope);
            if duel.deliver(false, &envelope).is_err() {
                break;
            }
            if duel.game(true).phase == PHASE_DONE {
                return;
            }
        }
    }

    #[test]
    fn manifest_is_complete_and_registry_keeps_builtins() {
        let router = lrgp_router();
        let ids: Vec<String> = router.list_apps().into_iter().map(|m| m.app_id).collect();
        for id in ["battleship", "chess", "four_in_a_row", "ttt"] {
            assert!(ids.contains(&id.to_string()), "missing {id}");
        }
        let manifest = BattleshipApp::new().manifest();
        assert_eq!(manifest.max_players, 2);
        for action in [CMD_CHALLENGE, CMD_ACCEPT, CMD_DECLINE, CMD_MOVE, CMD_RESIGN] {
            assert!(manifest.actions.iter().any(|a| a == action));
        }
        assert!(!manifest.actions.iter().any(|a| a.starts_with("draw")));
    }

    #[test]
    fn fleet_geometry_rejects_overlap_edges_and_wrong_sizes() {
        assert!(layout_from_fleet(&FLEET_A).is_some());
        assert!(layout_from_fleet(&FLEET_B).is_some());
        assert!(layout_from_fleet(&[0, 20, 40, 60]).is_none());
        // Carrier starting at column 6 runs off the right edge.
        assert!(layout_from_fleet(&[12, 20, 40, 60, 80]).is_none());
        // Carrier vertical from row 6 runs off the bottom.
        assert!(layout_from_fleet(&[121, 20, 40, 60, 80]).is_none());
        // Battleship placed on top of the carrier.
        assert!(layout_from_fleet(&[0, 2, 40, 60, 80]).is_none());
        assert!(layout_from_fleet(&[200, 20, 40, 60, 80]).is_none());
    }

    #[test]
    fn honest_game_completes_converges_and_stays_inside_the_wire_budget() {
        let mut duel = Duel::start();
        duel.ready();
        assert_eq!(duel.game(true).phase, PHASE_BATTLE);
        assert_eq!(duel.game(true).turn, A);
        assert_eq!(duel.game(false).turn, A);

        play_until_a_wins(&mut duel, &|_| {});
        assert_eq!(duel.game(false).phase, PHASE_REVEAL);
        assert!(duel.game(false).owes_reveal);
        duel.send(false, CMD_MOVE, reveal()).unwrap();
        assert_eq!(duel.game(true).phase, PHASE_REVEAL);
        assert!(duel.game(true).owes_reveal);
        duel.send(true, CMD_MOVE, reveal()).unwrap();

        for of_a in [true, false] {
            let session = duel.session(of_a);
            let game = Game::load(&session).unwrap();
            assert_eq!(session.status, STATUS_COMPLETED);
            assert_eq!(game.terminal, "win");
            assert_eq!(game.winner, A);
            assert_eq!(game.phase, PHASE_DONE);
            assert_eq!(game.their_board.len(), CELLS);
        }
        assert!(
            duel.max_envelope < ENVELOPE_MAX_PACKED,
            "{}",
            duel.max_envelope
        );
        // Terminal sessions accept nothing further.
        assert!(duel.send(true, CMD_MOVE, fire(60)).is_err());
    }

    #[test]
    fn secrets_never_appear_on_the_wire_before_the_reveal() {
        let mut duel = Duel::start();
        let envelope = duel.prepare(true, CMD_MOVE, commit(&FLEET_A)).unwrap();
        let bytes = lrgp::protocol::pack_to_bytes(&envelope).unwrap();
        let salt = duel.game(true).my_salt.clone();
        assert!(!salt.is_empty());
        assert!(!bytes.windows(salt.len()).any(|w| w == salt.as_slice()));
        assert!(!bytes.windows(5).any(|w| w == FLEET_A));
        duel.deliver(true, &envelope).unwrap();
        duel.send(false, CMD_MOVE, commit(&FLEET_B)).unwrap();
        let fire = duel.prepare(true, CMD_MOVE, fire(3)).unwrap();
        let bytes = lrgp::protocol::pack_to_bytes(&fire).unwrap();
        assert!(!bytes.windows(salt.len()).any(|w| w == salt.as_slice()));
    }

    #[test]
    fn illegal_intents_are_rejected_without_changing_state() {
        let mut duel = Duel::start();
        assert!(
            duel.send(true, CMD_MOVE, commit(&[0, 2, 40, 60, 80]))
                .is_err()
        );
        assert!(
            duel.send(true, CMD_MOVE, fire(3)).is_err(),
            "fire during setup"
        );
        assert!(duel.send(true, CMD_MOVE, reveal()).is_err());
        duel.ready();
        assert!(
            duel.send(true, CMD_MOVE, commit(&FLEET_A)).is_err(),
            "double commit"
        );
        assert!(duel.send(false, CMD_MOVE, fire(3)).is_err(), "not B's turn");
        assert!(duel.send(true, CMD_MOVE, fire(100)).is_err());
        duel.send(true, CMD_MOVE, fire(3)).unwrap();
        assert!(
            duel.send(true, CMD_MOVE, fire(4)).is_err(),
            "A fired twice in a row"
        );
        duel.send(false, CMD_MOVE, fire(3)).unwrap();
        assert!(
            duel.send(true, CMD_MOVE, fire(3)).is_err(),
            "cell already used"
        );
        assert_eq!(duel.game(true).move_count, 2);
    }

    #[test]
    fn first_shot_carries_the_commitment_so_reordering_cannot_strand_the_game() {
        let mut duel = Duel::start();
        duel.send(false, CMD_MOVE, commit(&FLEET_B)).unwrap();
        // A commits locally, but its commit message is delayed.
        let delayed = duel.prepare(true, CMD_MOVE, commit(&FLEET_A)).unwrap();
        let first_shot = duel.prepare(true, CMD_MOVE, fire(3)).unwrap();
        duel.deliver(true, &first_shot).unwrap();
        assert_eq!(duel.game(false).phase, PHASE_BATTLE);
        // The late commit is an idempotent no-op.
        duel.deliver(true, &delayed).unwrap();
        assert_eq!(duel.game(false).phase, PHASE_BATTLE);
        assert_eq!(duel.game(false).move_count, 1);
    }

    #[test]
    fn answers_no_legal_fleet_can_produce_end_the_game_for_the_verifier() {
        let mut duel = Duel::start();
        duel.ready();
        // A shoots only water; B claims a hit every time. After the 18th
        // claimed hit (or the 17th without sunk ships) no legal fleet fits.
        let mut a_targets = water_cells(&FLEET_B).into_iter();
        let mut b_targets = water_cells(&FLEET_A).into_iter();
        for _ in 0..40 {
            duel.send(true, CMD_MOVE, fire(a_targets.next().unwrap()))
                .unwrap();
            let mut envelope = duel
                .prepare(false, CMD_MOVE, fire(b_targets.next().unwrap()))
                .unwrap();
            if let Some(rmpv::Value::Map(pairs)) = envelope.get_mut("p") {
                for (key, value) in pairs.iter_mut() {
                    if key.as_str() == Some("a") {
                        *value = rmpv::Value::String("h".into());
                    }
                }
            }
            duel.deliver(false, &envelope).unwrap();
            if duel.game(true).phase == PHASE_DONE {
                break;
            }
        }
        let session = duel.session(true);
        let game = Game::load(&session).unwrap();
        assert_eq!(session.status, STATUS_COMPLETED);
        assert_eq!(game.terminal, "cheat");
        assert_eq!(game.winner, A);
        assert_eq!(game.hits_out(), SHIP_CELLS);
        assert!(game.cheat_reason.contains("not every ship"));
    }

    #[test]
    fn tampered_reveals_are_caught_by_the_commitment_and_the_answer_replay() {
        // Wrong salt: commitment mismatch.
        let mut duel = Duel::start();
        duel.ready();
        play_until_a_wins(&mut duel, &|_| {});
        let mut envelope = duel.prepare(false, CMD_MOVE, reveal()).unwrap();
        if let Some(rmpv::Value::Map(pairs)) = envelope.get_mut("p") {
            for (key, value) in pairs.iter_mut() {
                if key.as_str() == Some("z") {
                    *value = rmpv::Value::Binary(vec![9; SALT_BYTES]);
                }
            }
        }
        duel.deliver(false, &envelope).unwrap();
        let game = duel.game(true);
        assert_eq!(game.terminal, "cheat");
        assert_eq!(game.winner, A);
        assert!(game.cheat_reason.contains("commitment"));

        // A different but legal fleet whose hash is not the commitment.
        let mut duel = Duel::start();
        duel.ready();
        play_until_a_wins(&mut duel, &|_| {});
        let mut envelope = duel.prepare(false, CMD_MOVE, reveal()).unwrap();
        if let Some(rmpv::Value::Map(pairs)) = envelope.get_mut("p") {
            for (key, value) in pairs.iter_mut() {
                if key.as_str() == Some("b") {
                    *value = rmpv::Value::Binary(FLEET_A.to_vec());
                }
            }
        }
        duel.deliver(false, &envelope).unwrap();
        assert_eq!(duel.game(true).terminal, "cheat");
    }

    #[test]
    fn a_losing_peer_that_lied_about_one_answer_is_exposed_by_the_replay() {
        // Build a game state directly: B committed to FLEET_B but answered a
        // real hit as a miss. The honest replay check must catch it.
        let salt = [7u8; SALT_BYTES];
        let mut game = Game::new(A);
        game.their_commit = commitment("0123456789abcdef", B, &FLEET_B, &salt).to_vec();
        let hit_cell = ship_cells(&FLEET_B)[0];
        game.out_log.push(LogEntry {
            cell: hit_cell,
            answer: b'm',
            ship: -1,
        });
        let verdict = verify_reveal(&game, "0123456789abcdef", B, &FLEET_B, &salt);
        assert!(verdict.unwrap_err().contains("contradicts"));
        game.out_log[0].answer = b'h';
        assert!(verify_reveal(&game, "0123456789abcdef", B, &FLEET_B, &salt).is_ok());
        // A commitment is bound to its session and player.
        assert!(verify_reveal(&game, "fedcba9876543210", B, &FLEET_B, &salt).is_err());
        assert!(verify_reveal(&game, "0123456789abcdef", A, &FLEET_B, &salt).is_err());
    }

    #[test]
    fn resignation_ends_the_game_in_any_phase() {
        let mut duel = Duel::start();
        duel.send(true, CMD_RESIGN, HashMap::new()).unwrap();
        let (a, b) = (duel.game(true), duel.game(false));
        assert_eq!((a.terminal.as_str(), a.winner.as_str()), ("resign", B));
        assert_eq!((b.terminal.as_str(), b.winner.as_str()), ("resign", B));
        assert_eq!(duel.session(true).status, STATUS_COMPLETED);
    }

    #[test]
    fn every_intermediate_state_survives_persistence_restore() {
        let mut duel = Duel::start();
        let check = |duel: &Duel| {
            for of_a in [true, false] {
                let fresh = lrgp_router();
                fresh
                    .restore_session(duel.session(of_a))
                    .expect("restorable state");
            }
        };
        check(&duel);
        duel.ready();
        check(&duel);
        let mut a_targets = ship_cells(&FLEET_B).into_iter();
        let mut b_targets = water_cells(&FLEET_A).into_iter();
        for _ in 0..6 {
            duel.send(true, CMD_MOVE, fire(a_targets.next().unwrap()))
                .unwrap();
            check(&duel);
            duel.send(false, CMD_MOVE, fire(b_targets.next().unwrap()))
                .unwrap();
            check(&duel);
        }
        // A tampered fleet in storage is refused.
        let mut stored = duel.session(true);
        stored
            .metadata
            .insert("my_fleet".into(), serde_json::json!([0, 20, 40, 60, 82]));
        assert!(lrgp_router().restore_session(stored).is_err());
    }

    #[test]
    fn wire_payload_shapes_are_strict() {
        let app = BattleshipApp::new();
        let wire = |pairs: Vec<(&str, rmpv::Value)>| -> HashMap<String, rmpv::Value> {
            pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
        };
        let int = |n: u64| rmpv::Value::Integer(n.into());
        let text = |s: &str| rmpv::Value::String(s.into());
        let bin = |n: usize| rmpv::Value::Binary(vec![1; n]);
        let bad = [
            wire(vec![("k", text("c")), ("h", bin(31))]),
            wire(vec![("k", text("c")), ("h", bin(32)), ("x", int(1))]),
            wire(vec![("k", text("f")), ("n", int(1)), ("c", int(5))]),
            wire(vec![
                ("k", text("f")),
                ("n", int(2)),
                ("c", int(100)),
                ("a", text("m")),
            ]),
            wire(vec![
                ("k", text("f")),
                ("n", int(2)),
                ("c", int(5)),
                ("a", text("s")),
            ]),
            wire(vec![
                ("k", text("f")),
                ("n", int(2)),
                ("c", int(5)),
                ("a", text("m")),
                ("s", int(1)),
            ]),
            wire(vec![
                ("k", text("f")),
                ("n", int(2)),
                ("c", int(5)),
                ("a", text("z")),
            ]),
            wire(vec![
                ("k", text("r")),
                ("n", int(3)),
                ("b", bin(4)),
                ("z", bin(16)),
            ]),
            wire(vec![
                ("k", text("r")),
                ("n", int(3)),
                ("b", bin(5)),
                ("z", bin(15)),
            ]),
            wire(vec![
                ("k", text("r")),
                ("n", int(3)),
                ("a", text("h")),
                ("s", int(1)),
                ("b", bin(5)),
                ("z", bin(16)),
            ]),
            wire(vec![("k", text("q"))]),
        ];
        for payload in bad {
            let result = app.handle_incoming("0123456789abcdef", CMD_MOVE, &payload, B, A);
            assert!(result.error.is_some(), "accepted {payload:?}");
        }
    }
}
