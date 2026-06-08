use serde::{Deserialize, Serialize};

// outbound

#[derive(Debug, Serialize)]
pub struct HelloMsg {
    #[serde(rename = "type")]
    pub msg_type: &'static str,
    pub v: u32,
    pub uuid: String,
    pub nickname: String,
    pub code: String,
}

impl HelloMsg {
    pub fn new(uuid: &str, nickname: &str, code: &str) -> Self {
        Self {
            msg_type: "hello",
            v: 5,
            uuid: uuid.to_string(),
            nickname: nickname.to_string(),
            code: code.to_string(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ResultMsg {
    #[serde(rename = "type")]
    pub msg_type: &'static str,
    pub seed: String,
    pub total_done: u64,
    pub best_correct: i32,
    pub best_arr: Vec<u8>,
    pub best_index: u64,
}

impl ResultMsg {
    pub fn new(
        seed: &str,
        total_done: u64,
        best_correct: i32,
        best_arr: Vec<u8>,
        best_index: u64,
    ) -> Self {
        Self {
            msg_type: "result",
            seed: seed.to_string(),
            total_done,
            best_correct,
            best_arr,
            best_index,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct StopMsg {
    #[serde(rename = "type")]
    pub msg_type: &'static str,
}

impl StopMsg {
    pub fn new() -> Self {
        Self { msg_type: "stop" }
    }
}

// inbound

#[derive(Debug, Deserialize)]
pub struct ServerMsg {
    #[serde(rename = "type")]
    pub msg_type: String,

    #[serde(default)]
    pub uuid: Option<String>,
    #[serde(default)]
    pub nickname: Option<String>,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub lifetime_shuffles: Option<u64>,
    #[serde(default)]
    pub all_time_best: Option<u32>,

    #[serde(default)]
    pub seed: Option<String>,
    #[serde(default)]
    pub count: Option<u64>,

    #[serde(default)]
    pub credit: Option<u64>,
    #[serde(default)]
    pub rate: Option<u64>,
    #[serde(default)]
    pub session_best: Option<u32>,
    #[serde(default)]
    pub batch_best: Option<u32>,
    #[serde(default)]
    pub tick_best: Option<u32>,

    #[serde(default)]
    pub reason: Option<String>,

    #[serde(default)]
    pub expires_at: Option<u64>,

    #[serde(default)]
    pub batch_size: Option<u64>,
}
