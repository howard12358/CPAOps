use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Serialize)]
pub struct Output {
    pub ok: bool,
    pub code: u8,
    pub message: String,
    pub data: Value,
}

impl Output {
    pub fn success(message: impl Into<String>) -> Self {
        Self {
            ok: true,
            code: 0,
            message: message.into(),
            data: Value::Null,
        }
    }

    pub fn success_with_data(message: impl Into<String>, data: Value) -> Self {
        Self {
            ok: true,
            code: 0,
            message: message.into(),
            data,
        }
    }

    pub fn failure(code: u8, message: impl Into<String>) -> Self {
        Self {
            ok: false,
            code,
            message: message.into(),
            data: Value::Null,
        }
    }

    pub fn failure_with_data(code: u8, message: impl Into<String>, data: Value) -> Self {
        Self {
            ok: false,
            code,
            message: message.into(),
            data,
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self)
            .unwrap_or_else(|_| "{\"ok\":false,\"code\":1,\"message\":\"输出序列化失败\"}".into())
    }
}
