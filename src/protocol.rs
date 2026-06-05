use serde::{Deserialize, Serialize};

// outbound

#[derive(Debug, Serialize)]
pub struct HelloMsg {
    #[serde(rename = "type")]
    pub msg_type: &'static str,
    pub uuid: String,
    pub nickname: String,
    pub code: String,
}

impl HelloMsg {
    pub fn new(uuid: &str, nickname: &str, code: &str) -> Self {
        Self {
            msg_type: "hello",
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
    pub elapsed: f64,
}

impl ResultMsg {
    pub fn new(
        seed: &str,
        total_done: u64,
        best_correct: i32,
        best_arr: Vec<u8>,
        elapsed: f64,
    ) -> Self {
        Self {
            msg_type: "result",
            seed: seed.to_string(),
            total_done,
            best_correct,
            best_arr,
            elapsed,
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
    pub lifetime_shuffles: Option<u64>,
    #[serde(default)]
    pub all_time_best: Option<u32>,

    #[serde(default)]
    pub seed: Option<String>,
    #[serde(default)]
    pub batch_size: Option<u64>,

    #[serde(default)]
    pub credit: Option<u64>,
    #[serde(default)]
    pub rate: Option<u64>,
    #[serde(default)]
    pub session_best: Option<u32>,
    #[serde(default)]
    pub batch_best: Option<u32>,

    #[serde(default)]
    pub reason: Option<String>,
}
