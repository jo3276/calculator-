use serde::{Deserialize,Serialize};

#[derive(Serialize)]
pub struct HealthResponse (
    pub status: String,
)

#[derive(Deserialize)]
pub struct RegisterRequest {
    pub device_id: String,
}

#[derive(Serialize)]
pub struct RegisterResponse {
    pub message: String,
}

#[derive(Serialize)]
pub struct HeartbeatResponse {
    pub message: String,
}