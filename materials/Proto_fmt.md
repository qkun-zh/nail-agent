---

## 格式一：ACP 协议（客户端 ↔ Agent）

### 协议概述
- **全称**：Agent Client Protocol，由 Zed Industries 创建（2025.08），Apache 2.0。
- **用途**：代码编辑器 ↔ AI 编程 Agent 的双向通信，管理会话、提示、权限等。
- **传输**：标准为 **stdio**（子进程，JSON 行传输），也支持 Streamable HTTP（草案）。
- **基础**：**严格基于 JSON‑RPC 2.0**。所有消息必须包含 `jsonrpc: "2.0"`，请求带 `id`，通知无 `id`。
- **约定**：方法名使用 camelCase（如 `session/new`），参数名 camelCase（如 `sessionId`）。

---

### 请求结构（客户端 → Agent）
所有请求均为 JSON‑RPC 请求对象，包含 `method` 和 `params`。通用错误响应见响应部分。

#### `initialize`
```json
{
  "jsonrpc": "2.0",
  "id": "string | number",              // 必选
  "method": "initialize",
  "params": {
    "protocolVersion": "string",        // 必选，如 "2026-06-20"
    "clientInfo": {                     // 必选
      "name": "string",                 // 必选
      "version": "string"               // 可选
    },
    "capabilities": {                   // 可选，声明客户端支持的功能
      "session": {
        "list": true,                   // 布尔值
        "load": true,
        "close": true
      }
    }
  }
}
```

#### `authenticate`
```json
{
  "jsonrpc": "2.0",
  "id": "string | number",
  "method": "authenticate",
  "params": {
    "method": "string",                 // 必选，认证方式
    "credentials": {}                   // 必选，具体凭据
  }
}
```

#### `session/new`
```json
{
  "jsonrpc": "2.0",
  "id": "string | number",
  "method": "session/new",
  "params": {}                          // 空对象
}
```

#### `session/load`
```json
{
  "jsonrpc": "2.0",
  "id": "string | number",
  "method": "session/load",
  "params": {
    "sessionId": "string"               // 必选
  }
}
```

#### `session/list`
```json
{
  "jsonrpc": "2.0",
  "id": "string | number",
  "method": "session/list",
  "params": {
    "cursor": "string",                 // 可选，分页游标
    "limit": 10,                        // 可选，默认值由 Agent 决定
    "filter": {}                        // 可选，过滤条件
  }
}
```

#### `session/close`
```json
{
  "jsonrpc": "2.0",
  "id": "string | number",
  "method": "session/close",
  "params": {
    "sessionId": "string"               // 必选
  }
}
```

#### `prompt`
```json
{
  "jsonrpc": "2.0",
  "id": "string | number",
  "method": "prompt",
  "params": {
    "sessionId": "string",              // 必选
    "prompt": [                         // 必选，ContentBlock 数组
      {
        "type": "text",                 // 当前仅 "text"
        "text": "string"                // 必选
      }
    ]
  }
}
```

#### `$/cancel_request`
```json
{
  "jsonrpc": "2.0",
  "id": "string | number",
  "method": "$/cancel_request",
  "params": {
    "requestId": "string | number"      // 必选，要取消的请求 ID
  }
}
```

---

### 响应结构（Agent → 客户端）
所有响应均为 JSON‑RPC 响应对象，包含 `id` 和 `result` 或 `error`。

#### 成功响应模板
```json
{
  "jsonrpc": "2.0",
  "id": "原请求 id",
  "result": { ... }                     // 各方法专属结果
}
```

#### 错误响应模板
```json
{
  "jsonrpc": "2.0",
  "id": "原请求 id",
  "error": {
    "code": 1001,                       // 整数，业务错误码
    "message": "Session not found",     // 字符串
    "data": {}                          // 可选，附加信息
  }
}
```

#### `initialize` 响应 result
```json
{
  "protocolVersion": "string",          // 必选
  "serverInfo": {                       // 必选
    "name": "string",                   // 必选
    "version": "string"                 // 可选
  },
  "capabilities": {                     // 必选
    "session": {                        // 可选，是否支持会话管理
      "list": true,
      "load": true,
      "close": true
    }
  }
}
```

#### `session/new` 响应 result
```json
{
  "sessionId": "string"                 // 必选
}
```

#### `session/list` 响应 result
```json
{
  "sessions": [                         // 必选
    {
      "id": "string",
      "createdAt": "ISO8601 string",    // 可选
      "metadata": {}                    // 可选
    }
  ],
  "nextCursor": "string"                // 可选
}
```

#### `prompt` 响应 result
```json
{
  "stopReason": "end_turn"              // 当前仅 "end_turn"
}
```

其他 `session/load`、`session/close`、`$/cancel_request` 的 `result` 为空对象 `{}`。

---

### 通知结构（Agent → 客户端，主动推送）
通知为 JSON‑RPC 通知（无 `id`）。

#### `session/update`
```json
{
  "jsonrpc": "2.0",
  "method": "session/update",
  "params": {
    "sessionId": "string",              // 必选
    "update": {                         // 必选
      "UserMessageChunk": {             // 或 "AgentMessageChunk"
        "messageId": "string",          // 可选
        "content": {                    // 必选
          "type": "text",
          "text": "string"
        },
        "isFinal": false                // 可选，布尔值，表示消息块是否结束
      }
    }
  }
}
```

---

### 完整交互示例
仅展示关键消息模板，实际顺序由应用逻辑决定。

**初始化请求**：
```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2026-06-20","clientInfo":{"name":"my-editor"},"capabilities":{"session":{"list":true}}}}
```
**初始化响应**：
```json
{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2026-06-20","serverInfo":{"name":"agent"},"capabilities":{"session":{"list":true}}}}
```

**新建会话**：
```json
{"jsonrpc":"2.0","id":2,"method":"session/new","params":{}}
```
**会话响应**：
```json
{"jsonrpc":"2.0","id":2,"result":{"sessionId":"sess_123"}}
```

**发送提示**：
```json
{"jsonrpc":"2.0","id":3,"method":"prompt","params":{"sessionId":"sess_123","prompt":[{"type":"text","text":"1+2?"}]}}
```
**流式通知（块）**：
```json
{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"sess_123","update":{"AgentMessageChunk":{"messageId":"m1","content":{"type":"text","text":"3"},"isFinal":true}}}}
```
**提示响应**：
```json
{"jsonrpc":"2.0","id":3,"result":{"stopReason":"end_turn"}}
```

**错误响应示例**：
```json
{"jsonrpc":"2.0","id":3,"error":{"code":1001,"message":"Session not found"}}
```

---

## 格式二：OpenAI Chat Completion API（Agent ↔ LLM）

### 协议概述
- **端点**：`POST https://api.openai.com/v1/chat/completions`
- **认证**：`Authorization: Bearer <API Key>`
- **格式**：HTTPS + JSON，字段 snake_case。
- **支持**：非流式（普通 JSON）和流式（SSE），支持工具调用。

---

### 请求结构（`CreateChatCompletionRequest`）

```json
{
  "model": "string",                     // 必选
  "messages": [                          // 必选
    {
      "role": "system" | "user" | "assistant" | "tool",
      "content": "string | null",        // tool 和 user 必填，assistant 可为 null
      "name": "string",                  // 可选
      "tool_calls": [                    // 仅 assistant 角色使用
        {
          "id": "string",                // 必选
          "type": "function",            // 必选
          "function": {                  // 必选
            "name": "string",
            "arguments": "string"        // JSON 字符串
          }
        }
      ] | null,
      "tool_call_id": "string"           // 仅 tool 角色使用，必选
    }
  ],
  "tools": [                             // 可选
    {
      "type": "function",
      "function": {
        "name": "string",                // 必选
        "description": "string",         // 可选
        "parameters": {                  // 可选，JSON Schema 对象
          "type": "object",
          "properties": {},
          "required": []
        },
        "strict": false                  // 可选
      }
    }
  ],
  "tool_choice": "auto" | "required" | {"type":"function","function":{"name":"..."}}, // 可选
  "parallel_tool_calls": true,           // 可选，布尔
  "response_format": {"type": "json_object"}, // 可选
  "temperature": 1.0,                    // 可选，0~2
  "top_p": 1.0,                          // 可选
  "n": 1,                                // 可选
  "stream": false,                       // 可选，布尔
  "stop": null | string | [string],      // 可选
  "max_tokens": null | integer,          // 可选
  "presence_penalty": 0,                 // 可选，-2~2
  "frequency_penalty": 0,                // 可选，-2~2
  "logit_bias": null | object,           // 可选
  "seed": null | integer,                // 可选
  "user": "string"                       // 可选
}
```

---

### 响应结构（非流式，`ChatCompletionResponse`）

```json
{
  "id": "string",
  "object": "chat.completion",
  "created": 1234567890,
  "model": "string",
  "system_fingerprint": "string",        // 可选
  "choices": [                           // 必选
    {
      "index": 0,
      "message": {                       // 必选
        "role": "assistant",
        "content": "string | null",      // 无工具调用时为字符串，有则为 null
        "tool_calls": [                  // 可选，有工具调用时存在
          {
            "id": "string",
            "type": "function",
            "function": {
              "name": "string",
              "arguments": "string"      // JSON 字符串
            }
          }
        ] | null
      },
      "finish_reason": "stop" | "tool_calls" | "length" | "content_filter" | null,
      "logprobs": null                   // 可选
    }
  ],
  "usage": {                             // 可选
    "prompt_tokens": 0,
    "completion_tokens": 0,
    "total_tokens": 0
  }
}
```

### 流式响应（`stream: true`）
- 响应类型：`text/event-stream`。
- 每块为 `data: {"choices":[{"index":0,"delta":{...},"finish_reason":null}]}`。
- `delta` 可含 `role`、`content`、`tool_calls`（增量）。
- 结束标记：`data: [DONE]`。

### 错误响应
```json
{
  "error": {
    "message": "string",
    "type": "string",
    "param": "string | null",
    "code": "string | null"
  }
}
```

---

### 完整交互示例

**请求（强制工具调用）**：
```json
{
  "model": "gpt-4",
  "messages": [{"role": "user", "content": "1+2?"}],
  "tools": [{
    "type": "function",
    "function": {
      "name": "add",
      "parameters": {"type":"object","properties":{"a":{"type":"integer"},"b":{"type":"integer"}},"required":["a","b"]}
    }
  }],
  "tool_choice": "required"
}
```

**响应（工具调用）**：
```json
{
  "id": "chatcmpl-xxx",
  "object": "chat.completion",
  "created": 1710000000,
  "model": "gpt-4",
  "choices": [{
    "index": 0,
    "message": {
      "role": "assistant",
      "content": null,
      "tool_calls": [{"id":"call_1","type":"function","function":{"name":"add","arguments":"{\"a\":1,\"b\":2}"}}]
    },
    "finish_reason": "tool_calls"
  }],
  "usage": {"prompt_tokens": 50, "completion_tokens": 20, "total_tokens": 70}
}
```

**第二轮请求（附工具结果）**：
```json
{
  "model": "gpt-4",
  "messages": [
    {"role": "user", "content": "1+2?"},
    {"role": "assistant", "content": null, "tool_calls": [{"id":"call_1","type":"function","function":{"name":"add","arguments":"{\"a\":1,\"b\":2}"}}]},
    {"role": "tool", "tool_call_id": "call_1", "content": "3"}
  ],
  "tools": [...],
  "tool_choice": "auto"
}
```

**最终响应（纯文本）**：
```json
{
  "choices": [{
    "message": {"role":"assistant","content":"1 加 2 等于 3。"},
    "finish_reason": "stop"
  }]
}
```

---

## 格式三：MCP 协议（Agent ↔ MCP Server）

### 协议概述
- **全称**：Model Context Protocol，由 Anthropic 创建（2024.11）。
- **用途**：LLM 应用与外部数据源/工具的标准化集成。
- **基础**：**严格基于 JSON‑RPC 2.0**。
- **传输**：支持 **stdio**（子进程）和 **HTTP+SSE**。
- **生命周期**：必须经过初始化（`initialize` + `initialized` 通知）→ 正常运行 → 关闭。

---

### 请求结构（Agent → MCP Server）
所有请求均为 JSON‑RPC 请求。

#### `initialize`
```json
{
  "jsonrpc": "2.0",
  "id": "string | number",
  "method": "initialize",
  "params": {
    "protocolVersion": "0.1.0",         // 必选
    "capabilities": {                   // 可选，声明客户端能力
      "tools": {}
    },
    "clientInfo": {                     // 必选
      "name": "string",
      "version": "string"               // 可选
    }
  }
}
```

#### `tools/list`
```json
{
  "jsonrpc": "2.0",
  "id": "string | number",
  "method": "tools/list",
  "params": {
    "cursor": "string"                  // 可选，分页游标
  }
}
```

#### `tools/call`
```json
{
  "jsonrpc": "2.0",
  "id": "string | number",
  "method": "tools/call",
  "params": {
    "name": "string",                   // 必选，工具名
    "arguments": {}                     // 必选，键值对对象
  }
}
```

#### `resources/list`
```json
{
  "jsonrpc": "2.0",
  "id": "string | number",
  "method": "resources/list",
  "params": {
    "cursor": "string"                  // 可选
  }
}
```

#### `resources/read`
```json
{
  "jsonrpc": "2.0",
  "id": "string | number",
  "method": "resources/read",
  "params": {
    "uri": "string"                     // 必选，资源 URI
  }
}
```

#### `prompts/list`
```json
{
  "jsonrpc": "2.0",
  "id": "string | number",
  "method": "prompts/list",
  "params": {
    "cursor": "string"                  // 可选
  }
}
```

#### `prompts/get`
```json
{
  "jsonrpc": "2.0",
  "id": "string | number",
  "method": "prompts/get",
  "params": {
    "name": "string",                   // 必选
    "arguments": {}                     // 可选，模板参数
  }
}
```

---

### 响应结构（MCP Server → Agent）
所有响应均为 JSON‑RPC 响应，含 `result` 或 `error`。

#### 成功响应模板
```json
{
  "jsonrpc": "2.0",
  "id": "原请求 id",
  "result": { ... }
}
```

#### 错误响应模板（JSON‑RPC 标准错误码）
```json
{
  "jsonrpc": "2.0",
  "id": "原请求 id",
  "error": {
    "code": -32000,                     // 整数
    "message": "string",
    "data": {}                          // 可选
  }
}
```

#### `initialize` 响应 result
```json
{
  "protocolVersion": "0.1.0",
  "capabilities": {                     // 服务器能力
    "tools": {}
  },
  "serverInfo": {                       // 必选
    "name": "string",
    "version": "string"
  }
}
```

#### `tools/list` 响应 result
```json
{
  "tools": [                            // 必选
    {
      "name": "string",                 // 必选
      "description": "string",          // 可选
      "inputSchema": {                  // 必选，JSON Schema
        "type": "object",
        "properties": {},
        "required": []
      }
    }
  ],
  "nextCursor": "string"                // 可选
}
```

#### `tools/call` 响应 result
```json
{
  "content": [                          // 必选
    {
      "type": "text",                   // 当前仅 "text"
      "text": "string"                  // 必选
    }
  ],
  "isError": false                      // 可选，布尔值，表示工具执行是否出错
}
```

#### `resources/list` 响应 result
```json
{
  "resources": [
    {
      "uri": "string",                  // 必选
      "name": "string",                 // 必选
      "description": "string",          // 可选
      "mimeType": "string"              // 可选
    }
  ],
  "nextCursor": "string"
}
```

#### `resources/read` 响应 result
```json
{
  "contents": [                         // 必选
    {
      "uri": "string",
      "mimeType": "string",             // 可选
      "text": "string"                  // 与 blob 二选一
    }
  ]
}
```

#### `prompts/list` / `prompts/get` 响应 result
```json
{
  "prompts": [                          // list 返回数组，get 返回单个
    {
      "name": "string",
      "description": "string",          // 可选
      "arguments": [                    // 可选
        {
          "name": "string",
          "description": "string",
          "required": false
        }
      ]
    }
  ],
  "nextCursor": "string"                // 仅 list 有
}
```

---

### 通知结构（MCP Server → Agent，主动推送）
通知为 JSON‑RPC 通知（无 `id`）。

#### `notifications/progress`
```json
{
  "jsonrpc": "2.0",
  "method": "notifications/progress",
  "params": {
    "progressToken": "string",          // 必选
    "progress": 50,                     // 必选，当前进度
    "total": 100                        // 可选，总量
  }
}
```

#### `notifications/initialized`（客户端发送，表示初始化完成）
```json
{
  "jsonrpc": "2.0",
  "method": "notifications/initialized"
}
```

---

### 完整交互示例

**初始化**：
- 请求：`{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"0.1.0","clientInfo":{"name":"agent"}}}`
- 响应：`{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"0.1.0","capabilities":{"tools":{}},"serverInfo":{"name":"math-server"}}}`
- 通知：`{"jsonrpc":"2.0","method":"notifications/initialized"}`

**列出工具**：
- 请求：`{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}`
- 响应：`{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"add","inputSchema":{"type":"object","properties":{"a":{"type":"integer"},"b":{"type":"integer"}},"required":["a","b"]}}]}}`

**调用工具**：
- 请求：`{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"add","arguments":{"a":1,"b":2}}}`
- 响应（成功）：`{"jsonrpc":"2.0","id":3,"result":{"content":[{"type":"text","text":"3"}],"isError":false}}`
- 响应（工具执行错误）：`{"jsonrpc":"2.0","id":3,"result":{"content":[{"type":"text","text":"Division by zero"}],"isError":true}}`

**协议级错误（方法不存在）**：
```json
{"jsonrpc":"2.0","id":3,"error":{"code":-32601,"message":"Method not found"}}
```
