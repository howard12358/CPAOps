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

    pub fn human_message(&self) -> String {
        if let Some(checks) = self.data.get("checks").and_then(Value::as_array) {
            let mut output = self.message.clone();
            for check in checks {
                let name = check
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("未知检查");
                let level = match check.get("level").and_then(Value::as_str) {
                    Some("pass") => "通过",
                    Some("warning") => "警告",
                    Some("skipped") => "跳过",
                    _ => "失败",
                };
                let message = check.get("message").and_then(Value::as_str).unwrap_or("");
                output.push_str(&format!("\n{level} {name}：{message}"));
                if let Some(suggestion) = check.get("suggestion").and_then(Value::as_str) {
                    output.push_str(&format!("\n  建议：{suggestion}"));
                }
            }
            return output;
        }

        let Some(services) = self.data.get("services").and_then(Value::as_array) else {
            return self.message.clone();
        };

        let mut output = self.message.clone();
        for service in services {
            let name = service
                .get("service")
                .and_then(Value::as_str)
                .unwrap_or("未知服务");
            output.push('\n');
            output.push_str(name);
            output.push('：');
            if let Some(status) = service.get("status").and_then(Value::as_str) {
                if status == "已禁用" {
                    output.push_str("已停止（已禁用自动拉起）");
                } else {
                    output.push_str(status);
                }
                if let Some(port) = service.get("port").and_then(Value::as_u64) {
                    output.push_str("（端口 ");
                    output.push_str(&port.to_string());
                    if let Some(version) = service.get("version").and_then(Value::as_str) {
                        output.push_str("，版本 ");
                        output.push_str(version);
                    }
                    output.push('）');
                }
            } else if service.get("ok").and_then(Value::as_bool) == Some(true) {
                let version = service.get("version").and_then(Value::as_str);
                if service.get("state").and_then(Value::as_str) == Some("up_to_date") {
                    output.push_str("已是最新版本");
                } else {
                    output.push_str("成功");
                }
                if let Some(version) = version {
                    output.push('（');
                    output.push_str(version);
                    output.push('）');
                }
            } else {
                output.push_str("失败");
                if let Some(message) = service.get("message").and_then(Value::as_str) {
                    output.push('（');
                    output.push_str(message);
                    output.push('）');
                }
            }
        }
        output
    }
}
