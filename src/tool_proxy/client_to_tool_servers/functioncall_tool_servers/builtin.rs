use std::sync::Arc;

use rmcp::model::{CallToolResult, Content, Tool};
use serde_json::json;

use crate::tool_proxy::client_to_tool_servers::{ClientConfig, SimpleTool, ToolHandler};

pub fn config() -> ClientConfig {
    ClientConfig::function_call("builtin", tools())
}

pub fn tools() -> Vec<Arc<dyn ToolHandler>> {
    vec![
        Arc::new(add_tool()),
        Arc::new(multiply_tool()),
        Arc::new(time_tool()),
        Arc::new(uuid_tool()),
    ]
}

fn add_tool() -> SimpleTool {
    SimpleTool::new(
        Tool::new(
            "add",
            "add two numbers",
            json!({
                "type": "object",
                "properties": {
                    "a": {"type": "number", "description": "first number"},
                    "b": {"type": "number", "description": "second number"}
                },
                "required": ["a", "b"]
            })
            .as_object()
            .unwrap()
            .clone(),
        ),
        |args| {
            let a = args.get("a").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let b = args.get("b").and_then(|v| v.as_f64()).unwrap_or(0.0);
            Ok(CallToolResult::success(vec![Content::text(
                (a + b).to_string(),
            )]))
        },
    )
}

fn multiply_tool() -> SimpleTool {
    SimpleTool::new(
        Tool::new(
            "multiply",
            "multiply two numbers",
            json!({
                "type": "object",
                "properties": {
                    "a": {"type": "number", "description": "first number"},
                    "b": {"type": "number", "description": "second number"}
                },
                "required": ["a", "b"]
            })
            .as_object()
            .unwrap()
            .clone(),
        ),
        |args| {
            let a = args.get("a").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let b = args.get("b").and_then(|v| v.as_f64()).unwrap_or(0.0);
            Ok(CallToolResult::success(vec![Content::text(
                (a * b).to_string(),
            )]))
        },
    )
}

fn time_tool() -> SimpleTool {
    SimpleTool::new(
        Tool::new(
            "time",
            "get current date and time",
            json!({
                "type": "object",
                "properties": {
                    "format": {
                        "type": "string",
                        "description": "time format: iso / date / time / unix",
                        "enum": ["iso", "date", "time", "unix"]
                    }
                }
            })
            .as_object()
            .unwrap()
            .clone(),
        ),
        |args| {
            let fmt = args.get("format").and_then(|v| v.as_str()).unwrap_or("iso");
            let now = chrono::Local::now();
            let result = match fmt {
                "date" => now.format("%Y-%m-%d").to_string(),
                "time" => now.format("%H:%M:%S").to_string(),
                "unix" => now.timestamp().to_string(),
                _ => now.to_rfc3339(),
            };
            Ok(CallToolResult::success(vec![Content::text(result)]))
        },
    )
}

fn uuid_tool() -> SimpleTool {
    SimpleTool::new(
        Tool::new(
            "uuid",
            "generate a UUID (default v7, time-ordered)",
            json!({
                "type": "object",
                "properties": {
                    "count": {
                        "type": "integer",
                        "description": "generation count (default 1, max 10)",
                        "default": 1
                    }
                }
            })
            .as_object()
            .unwrap()
            .clone(),
        ),
        |args| {
            let count = args
                .get("count")
                .and_then(|v| v.as_i64())
                .unwrap_or(1)
                .clamp(1, 10);
            let uuids: Vec<String> = (0..count)
                .map(|_| uuid::Uuid::now_v7().to_string())
                .collect();
            Ok(CallToolResult::success(vec![Content::text(
                uuids.join("\n"),
            )]))
        },
    )
}
