use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum ApiResponse<T> {
    Ok { data: T },
    Error { message: String },
}
