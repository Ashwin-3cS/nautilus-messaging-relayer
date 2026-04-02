use crate::state::AppState;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_repr::{Deserialize_repr, Serialize_repr};
use std::collections::VecDeque;
use std::sync::Mutex;
use nautilus_enclave::EnclaveKeyPair;
use tracing::info;

// ── In-memory log ring buffer ─────────────────────────────────────────

pub struct LogBuffer {
    lines: Mutex<VecDeque<String>>,
    capacity: usize,
}

impl LogBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            lines: Mutex::new(VecDeque::with_capacity(capacity)),
            capacity,
        }
    }

    pub fn push(&self, line: String) {
        let mut buf = self.lines.lock().unwrap();
        if buf.len() >= self.capacity {
            buf.pop_front();
        }
        buf.push_back(line);
    }

    pub fn recent(&self, n: usize) -> Vec<String> {
        let buf = self.lines.lock().unwrap();
        buf.iter().rev().take(n).rev().cloned().collect()
    }
}

// ── Intent message types (matches Sui on-chain IntentMessage<T>) ──────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct IntentMessage<T: Serialize + Clone> {
    pub intent: IntentScope,
    pub timestamp_ms: u64,
    pub data: T,
}

#[derive(Serialize_repr, Deserialize_repr, Debug, Clone)]
#[repr(u8)]
pub enum IntentScope {
    Generic = 0,
    SignName = 1,
    MessageDelivery = 2,
}

#[derive(Serialize, Deserialize)]
pub struct ProcessedDataResponse<T> {
    pub response: T,
    pub signature: String,
    pub enclave_public_key: String,
}

/// BCS-encode the IntentMessage<T> and sign it with the enclave keypair.
pub fn to_signed_response<T: Serialize + Clone>(
    kp: &EnclaveKeyPair,
    payload: T,
    timestamp_ms: u64,
    intent: IntentScope,
) -> ProcessedDataResponse<IntentMessage<T>> {
    let intent_msg = IntentMessage {
        intent,
        timestamp_ms,
        data: payload,
    };
    let signing_payload = bcs::to_bytes(&intent_msg).expect("bcs serialization should not fail");
    let sig = kp.sign(&signing_payload);
    ProcessedDataResponse {
        response: intent_msg,
        signature: hex::encode(sig.to_bytes()),
        enclave_public_key: hex::encode(kp.public_key_bytes()),
    }
}

// ── Enclave error type ────────────────────────────────────────────────

#[derive(Debug)]
pub enum EnclaveError {
    GenericError(String),
}

impl IntoResponse for EnclaveError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            EnclaveError::GenericError(e) => (StatusCode::INTERNAL_SERVER_ERROR, e),
        };
        (status, Json(json!({ "error": message }))).into_response()
    }
}

// ── GET /get_attestation ──────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct GetAttestationResponse {
    pub attestation: String,
}

pub async fn get_attestation(
    State(state): State<AppState>,
) -> Result<Json<GetAttestationResponse>, EnclaveError> {
    info!("get_attestation called");
    let pk_bytes = state.eph_kp.public_key_bytes();
    let doc = nautilus_enclave::get_attestation(&pk_bytes, &[])
        .map_err(|e| EnclaveError::GenericError(format!("attestation failed: {}", e)))?;
    Ok(Json(GetAttestationResponse {
        attestation: doc.raw_cbor_hex,
    }))
}

// ── GET /health (enclave) ─────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct EnclaveHealthResponse {
    pub public_key: String,
    pub status: String,
}

pub async fn enclave_health(
    State(state): State<AppState>,
) -> Result<Json<EnclaveHealthResponse>, EnclaveError> {
    Ok(Json(EnclaveHealthResponse {
        public_key: hex::encode(state.eph_kp.public_key_bytes()),
        status: "ok".to_string(),
    }))
}

// ── GET /logs ─────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct LogsQueryParams {
    pub lines: Option<usize>,
}

#[derive(Serialize)]
pub struct LogsResponse {
    pub lines: Vec<String>,
    pub count: usize,
}

pub async fn get_logs(
    State(state): State<AppState>,
    Query(params): Query<LogsQueryParams>,
) -> Result<Json<LogsResponse>, EnclaveError> {
    let n = params.lines.unwrap_or(100).min(1000);
    let lines = state.logs.recent(n);
    Ok(Json(LogsResponse {
        count: lines.len(),
        lines,
    }))
}
