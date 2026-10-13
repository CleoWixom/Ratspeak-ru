//! A propagated game action is prepared on an isolated router. The shared
//! mutation guard spans the canonical read, receipt transaction and publication;
//! failed/cancelled preparations drop isolated state without rolling back a
//! different operation's live session.
use std::collections::HashMap;

use lrgp::app_base::IncomingDispatch;
use lrgp::protocol::{self, Envelope};
use lrgp::session::Session;
use lxmf_core::admission::{ProcessingMemoryLease, SuppressionReason};
use lxmf_core::message_api::LxMessage;
use ratspeak_db::admission::{AdmissionDbError, EffectCommit, GameReference};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::json;
use tokio::sync::OwnedMutexGuard;

use crate::{AppState, db};

// Historical game metadata is separate from the downloaded wire image. Reserve
// its complete bounded preparation allowance before reading/decoding it. This
// is a Rust buffer allowance, not a claim about SQLite's native allocator.
const MAX_SESSION_METADATA: usize = 64 * 1024;
const GAME_PROCESSING_ALLOWANCE: usize = 8 * 1024 * 1024;

#[derive(PartialEq)]
struct SessionRevision {
    app: String,
    version: u32,
    contact: String,
    initiator: String,
    status: String,
    metadata: String,
    created: u64,
    updated: u64,
    action: u64,
}

fn session(
    connection: &Connection,
    sid: &str,
    identity: &str,
) -> Result<Option<(Session, SessionRevision)>, AdmissionDbError> {
    // Inspect every variable column length before asking rusqlite to allocate
    // it. Private database corruption must not bypass the adapter's allowance.
    let largest: Option<i64> = connection.query_row(
        "SELECT max(length(CAST(metadata AS BLOB)),length(CAST(app_id AS BLOB)),length(CAST(contact_hash AS BLOB)),length(CAST(initiator AS BLOB)),length(CAST(status AS BLOB))) FROM app_sessions WHERE session_id=?1 AND identity_id=?2",
        params![sid,identity], |row|row.get(0)).optional()?;
    if largest.is_some_and(|length| length < 0 || length as usize > MAX_SESSION_METADATA) {
        return Err(AdmissionDbError::TooLarge);
    }
    let row = connection.query_row(
        "SELECT app_id,app_version,contact_hash,initiator,status,metadata,unread,created_at,updated_at,last_action_at FROM app_sessions WHERE session_id=?1 AND identity_id=?2",
        params![sid,identity], |row| Ok((row.get::<_,String>(0)?,row.get::<_,u32>(1)?,row.get::<_,String>(2)?,row.get::<_,String>(3)?,row.get::<_,String>(4)?,row.get::<_,String>(5)?,row.get::<_,i64>(6)?,row.get::<_,f64>(7)?,row.get::<_,f64>(8)?,row.get::<_,f64>(9)?)))
        .optional()?;
    row.map(
        |(app, version, contact, initiator, status, metadata, unread, created, updated, action)| {
            if !created.is_finite() || !updated.is_finite() || !action.is_finite() {
                return Err(AdmissionDbError::MissingEffect);
            }
            let decoded =
                serde_json::from_str(&metadata).map_err(|_| AdmissionDbError::MissingEffect)?;
            Ok((
                Session {
                    session_id: sid.into(),
                    identity_id: identity.into(),
                    app_id: app.clone(),
                    app_version: version,
                    contact_hash: contact.clone(),
                    initiator: initiator.clone(),
                    status: status.clone(),
                    metadata: decoded,
                    unread,
                    created_at: created,
                    updated_at: updated,
                    last_action_at: action,
                },
                SessionRevision {
                    app,
                    version,
                    contact,
                    initiator,
                    status,
                    metadata,
                    created: created.to_bits(),
                    updated: updated.to_bits(),
                    action: action.to_bits(),
                },
            ))
        },
    )
    .transpose()
}

fn retained_envelope(
    connection: &Connection,
    sid: &str,
    identity: &str,
    nonce: &[u8; 8],
    sender: &str,
) -> Result<Option<Vec<u8>>, AdmissionDbError> {
    let mut query=connection.prepare("SELECT length(envelope_mp),envelope_mp,sender FROM app_actions WHERE session_id=?1 AND identity_id=?2 AND envelope_mp IS NOT NULL")?;
    let mut rows = query.query(params![sid, identity])?;
    while let Some(row) = rows.next()? {
        let length: usize = row.get(0)?;
        if length > protocol::ENVELOPE_MAX_PACKED {
            return Err(AdmissionDbError::MissingEffect);
        }
        let bytes: Vec<u8> = row.get(1)?;
        let envelope =
            protocol::unpack_from_bytes(&bytes).map_err(|_| AdmissionDbError::MissingEffect)?;
        let validated =
            protocol::validate_envelope(&envelope).map_err(|_| AdmissionDbError::MissingEffect)?;
        if &validated.nonce == nonce {
            let prior_sender: String = row.get(2)?;
            if prior_sender != sender || validated.session_id != sid {
                return Err(AdmissionDbError::MissingEffect);
            }
            return Ok(Some(bytes));
        }
    }
    Ok(None)
}

fn challenge_capacity(
    connection: &Connection,
    identity: &str,
    sender: &str,
) -> Result<bool, AdmissionDbError> {
    let (total,participant):(usize,usize)=connection.query_row(
        "SELECT COUNT(*),COALESCE(SUM(contact_hash=?2),0) FROM app_sessions WHERE identity_id=?1 AND status='pending'",
        params![identity,sender],|row|Ok((row.get(0)?,row.get(1)?)))?;
    Ok(total < lrgp::constants::PENDING_SESSIONS_PER_IDENTITY_MAX
        && participant < lrgp::constants::PENDING_SESSIONS_PER_PARTICIPANT_MAX)
}

pub(super) fn decode(
    msg: &LxMessage,
    authenticated: bool,
) -> Result<Option<Envelope>, SuppressionReason> {
    let Some(marker) = msg.get_field(protocol::FIELD_CUSTOM_TYPE) else {
        return Ok(None);
    };
    // A non-LRGP custom field remains the caller's ordinary extension path.
    if marker.encoded().len() > 32 {
        return Ok(None);
    }
    let value = rmpv::decode::read_value(&mut marker.encoded())
        .map_err(|_| SuppressionReason::Malformed)?;
    if !matches!(&value,rmpv::Value::String(v) if v.as_str()==Some(protocol::PROTOCOL_TYPE)) {
        return Ok(None);
    }
    if !authenticated {
        return Err(SuppressionReason::InvalidSignature);
    }
    let meta = msg
        .get_field(protocol::FIELD_CUSTOM_META)
        .ok_or(SuppressionReason::Malformed)?;
    if meta.encoded().len() > protocol::ENVELOPE_MAX_PACKED {
        return Err(SuppressionReason::TooLarge);
    }
    meta.unique_map()
        .map_err(|_| SuppressionReason::Malformed)?;
    let data =
        rmpv::decode::read_value(&mut meta.encoded()).map_err(|_| SuppressionReason::Malformed)?;
    let fields = HashMap::from([
        (protocol::FIELD_CUSTOM_TYPE, value),
        (protocol::FIELD_CUSTOM_META, data),
    ]);
    protocol::unpack_envelope(&fields).map_err(|_| SuppressionReason::Malformed)
}

pub(super) struct PreparedGame {
    identity: String,
    sender: String,
    app: String,
    sid: String,
    command: String,
    nonce: [u8; 8],
    timestamp: f64,
    envelope: Vec<u8>,
    retained: Vec<u8>,
    payload: String,
    before: Option<SessionRevision>,
    next: Option<Session>,
    metadata: String,
    remote_error: Option<serde_json::Value>,
    replay: bool,
    applied: bool,
    published: bool,
    had_session: bool,
    _memory: ProcessingMemoryLease,
    _transaction: Option<OwnedMutexGuard<()>>,
}
impl PreparedGame {
    /// `identity` is the LXMF delivery destination, not the enclosing Reticulum
    /// identity used by the download receipt scope.
    pub(super) fn prepare(
        state: &AppState,
        msg: &LxMessage,
        identity: &str,
        authenticated: bool,
    ) -> Result<Option<Self>, SuppressionReason> {
        let Some(envelope) = decode(msg, authenticated)? else {
            return Ok(None);
        };
        if identity != hex::encode(msg.destination_hash()) {
            return Err(SuppressionReason::WrongDestination);
        }
        let transaction = state
            .lrgp_transaction
            .clone()
            .try_lock_owned()
            .map_err(|_| SuppressionReason::WorkerUnavailable)?;
        let memory = ProcessingMemoryLease::try_acquire(GAME_PROCESSING_ALLOWANCE)?;
        let valid =
            protocol::validate_envelope(&envelope).map_err(|_| SuppressionReason::Malformed)?;
        let sender = hex::encode(msg.source_hash());
        let connection = state
            .db
            .get()
            .map_err(|_| SuppressionReason::StorageUnavailable)?;
        let prior = session(&connection, &valid.session_id, identity)
            .map_err(|error| super::reason(&error))?;
        // SID namespace and participant binding remain global across apps.
        if prior.as_ref().is_some_and(|(s, _)| {
            s.app_id != valid.app_id
                || s.app_version != valid.version
                || (!s.contact_hash.is_empty() && s.contact_hash != sender)
        }) {
            return Err(SuppressionReason::Malformed);
        }
        let retained = retained_envelope(
            &connection,
            &valid.session_id,
            identity,
            &valid.nonce,
            &sender,
        )
        .map_err(|error| super::reason(&error))?;
        let replay = retained.is_some();
        if !replay
            && prior.is_none()
            && valid.command == protocol::CMD_CHALLENGE
            && !challenge_capacity(&connection, identity, &sender)
                .map_err(|error| super::reason(&error))?
        {
            return Err(SuppressionReason::Quota);
        }
        drop(connection);
        let working = lrgp::router::LrgpRouter::with_builtin_apps();
        if let Some((session, _)) = &prior {
            working
                .restore_session(session.clone())
                .map_err(|_| SuppressionReason::Malformed)?;
        }
        let (next, payload, remote_error) = if replay {
            (None, "{}".to_string(), None)
        } else {
            match working.dispatch_incoming(&envelope, &sender, identity) {
                Ok(IncomingDispatch::Applied(result)) if result.error.is_none() => {
                    let next = working
                        .snapshot_session(&valid.app_id, &valid.session_id, identity)
                        .ok_or(SuppressionReason::Malformed)?;
                    let payload = db::admission::bounded_json(
                        &result.emit.unwrap_or_default(),
                        MAX_SESSION_METADATA,
                    )
                    .map_err(|error| super::reason(&error))?;
                    (Some(next), payload, None)
                }
                Ok(IncomingDispatch::RemoteError(error)) => {
                    let payload =
                        json!({"code":error.code,"msg":error.message,"ref":error.reference});
                    (
                        None,
                        db::admission::bounded_json(&payload, MAX_SESSION_METADATA)
                            .map_err(|error| super::reason(&error))?,
                        Some(payload),
                    )
                }
                // A game action may depend on an earlier action still in the
                // node. Retain it for retry; never turn an engine/parser failure
                // into a durable discard merely to drain the node.
                Ok(IncomingDispatch::Applied(_)) => {
                    return Err(SuppressionReason::WorkerUnavailable);
                }
                Ok(IncomingDispatch::Replay) => return Err(SuppressionReason::WorkerUnavailable),
                Err(lrgp::protocol::LrgpError::SessionNotFound(_)) => {
                    return Err(SuppressionReason::WorkerUnavailable);
                }
                Err(lrgp::protocol::LrgpError::AdmissionLimit { .. }) => {
                    return Err(SuppressionReason::Quota);
                }
                Err(_) => return Err(SuppressionReason::Malformed),
            }
        };
        let metadata = next
            .as_ref()
            .map(|s| db::admission::bounded_json(&s.metadata, MAX_SESSION_METADATA))
            .transpose()
            .map_err(|error| super::reason(&error))?
            .unwrap_or_default();
        let packed =
            protocol::pack_to_bytes(&envelope).map_err(|_| SuppressionReason::Malformed)?;
        let retained = retained.unwrap_or_else(|| packed.clone());
        Ok(Some(Self {
            identity: identity.into(),
            sender,
            app: valid.app_id,
            sid: valid.session_id,
            command: valid.command,
            nonce: valid.nonce,
            timestamp: msg.timestamp(),
            envelope: packed,
            retained,
            payload,
            before: prior.map(|(_, revision)| revision),
            next,
            metadata,
            remote_error,
            replay,
            applied: false,
            published: false,
            had_session: false,
            _memory: memory,
            _transaction: Some(transaction),
        }))
    }

    pub(super) fn binding_lengths(&self) -> Vec<usize> {
        // Count repeated SQL bindings as repeated copies, including nonce
        // scans and post-write canonical reload, not only visible game text.
        let mut lengths = vec![self.envelope.len(), self.payload.len(), self.metadata.len()];
        for _ in 0..12 {
            lengths.extend([
                self.identity.len(),
                self.sid.len(),
                self.sender.len(),
                self.app.len(),
                self.command.len(),
            ]);
        }
        if let Some(session) = &self.next {
            lengths.extend([
                session.contact_hash.len(),
                session.initiator.len(),
                session.status.len(),
            ]);
        }
        lengths
    }
    pub(super) fn effect_reference(&self) -> GameReference {
        GameReference {
            identity: self.identity.clone(),
            session: self.sid.clone(),
            nonce: self.nonce,
            envelope: self.retained.clone(),
        }
    }
    pub(super) fn apply(
        &mut self,
        connection: &Connection,
    ) -> Result<EffectCommit, AdmissionDbError> {
        let current = session(connection, &self.sid, &self.identity)?;
        if current.as_ref().map(|(_, revision)| revision) != self.before.as_ref() {
            return Err(AdmissionDbError::Busy);
        }
        if let Some(retained) = retained_envelope(
            connection,
            &self.sid,
            &self.identity,
            &self.nonce,
            &self.sender,
        )? {
            if retained != self.retained {
                return Err(AdmissionDbError::Busy);
            }
            self.replay = true;
            return Ok(EffectCommit::Applied("game"));
        }
        if self.replay {
            return Err(AdmissionDbError::MissingEffect);
        }
        if self.before.is_none()
            && self.command == protocol::CMD_CHALLENGE
            && !challenge_capacity(connection, &self.identity, &self.sender)?
        {
            return Err(AdmissionDbError::Busy);
        }
        self.had_session = db::persist_inbound_game_action_conn(
            connection,
            &self.sid,
            &self.identity,
            &self.command,
            &self.payload,
            &self.sender,
            self.timestamp,
            &self.envelope,
            self.next.as_ref(),
        )
        .ok_or(AdmissionDbError::MissingEffect)?;
        // SQL owns unread/action timestamps; publish exactly the canonical
        // committed snapshot, including an observational remote-error action.
        self.next = session(connection, &self.sid, &self.identity)?.map(|(session, _)| session);
        self.applied = true;
        Ok(EffectCommit::Applied("game"))
    }
    /// Call only after the enclosing receipt transaction reports committed,
    /// including when an async generation was cancelled while SQLite ran.
    pub(super) fn committed(&mut self, state: &AppState) -> Result<(), AdmissionDbError> {
        if self.applied {
            if let Some(session) = self.next.take() {
                state.lrgp_router.restore_session(session).map_err(|e| {
                    AdmissionDbError::Unavailable(format!(
                        "committed game could not be hydrated: {e}"
                    ))
                })?;
            }
            self.published = true;
        }
        self._transaction.take();
        Ok(())
    }
    /// The caller has already checked the current identity generation and
    /// accepted the matching durable download token before emitting effects.
    pub(super) fn emit(&self, state: &AppState) {
        if !self.published || self.replay {
            return;
        }
        let all = db::list_game_sessions(&state.db, &self.identity, None, None);
        if let Some(error) = &self.remote_error {
            state.emit_to_all("game_protocol_error",json!({"session_id":self.sid,"app_id":self.app,"from":self.sender,"code":error["code"],"message":error["msg"],"ref":error["ref"]}));
        } else {
            let sessions =
                db::list_game_sessions(&state.db, &self.identity, Some(&self.sender), None);
            crate::notify_game_if_background(
                state,
                &self.sender,
                &self.sid,
                &self.app,
                &self.command,
                !self.had_session,
            );
            state.emit_to_all("active_games", json!({"hash":self.sender,"games":sessions}));
        }
        state.emit_to_all("all_game_sessions", all.into());
        state.emit_to_all("game_action_received",json!({"session_id":self.sid,"app_id":self.app,"command":self.command,"from":self.sender,"applied":self.remote_error.is_none(),"remote_error":self.remote_error.is_some()}));
    }
}
