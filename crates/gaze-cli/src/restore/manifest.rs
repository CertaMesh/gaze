use serde::{Deserialize, Serialize};

use crate::error::RestoreWarning;

#[derive(Deserialize)]
pub(crate) struct RestoreRequest {
    pub(crate) session_blob: String,
    pub(crate) text: String,
}

#[derive(Serialize)]
pub(crate) struct RestoreResponse {
    pub(crate) text: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) restore_warning: Vec<RestoreWarning>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) restore_telemetry: Option<gaze::RestoreTelemetry>,
}
