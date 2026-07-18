// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub const JSON_RPC_VERSION: &str = "2.0";
pub const TOOLING_PROTOCOL_VERSION: &str = "0.1.0-preview.1";
pub const MCP_PROTOCOL_VERSION: &str = "2025-11-25";

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RpcRequest {
    pub jsonrpc: String,
    #[serde(
        default,
        deserialize_with = "deserialize_rpc_id",
        skip_serializing_if = "Option::is_none"
    )]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

fn deserialize_rpc_id<'de, D>(deserializer: D) -> Result<Option<Value>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    if value.is_null() || value.is_string() || value.is_number() {
        Ok(Some(value))
    } else {
        Err(D::Error::custom(
            "JSON-RPC id must be a string, number, or null",
        ))
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(untagged)]
pub enum RpcResponse {
    Success {
        jsonrpc: String,
        id: Value,
        result: Value,
    },
    Failure {
        jsonrpc: String,
        id: Value,
        error: RpcError,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Clone, Debug)]
pub struct RpcHost {
    host_version: String,
    methods: BTreeSet<String>,
    features: BTreeSet<String>,
    negotiated: bool,
}

impl RpcHost {
    pub fn new(host_version: impl Into<String>) -> Self {
        Self {
            host_version: host_version.into(),
            methods: BTreeSet::from([
                "pliego/handshake".to_owned(),
                "pliego/diagnostics".to_owned(),
            ]),
            features: BTreeSet::new(),
            negotiated: false,
        }
    }

    pub fn with_feature(mut self, feature: impl Into<String>) -> Self {
        self.features.insert(feature.into());
        self
    }

    pub fn handle(&mut self, request: RpcRequest) -> Option<RpcResponse> {
        let id = request.id?;
        if request.jsonrpc != JSON_RPC_VERSION {
            return Some(failure(id, -32600, "Invalid Request", None));
        }
        if request.method == "pliego/handshake" {
            let requested = request
                .params
                .as_ref()
                .and_then(Value::as_object)
                .and_then(|params| params.get("protocolVersion"))
                .and_then(Value::as_str);
            if requested != Some(TOOLING_PROTOCOL_VERSION) {
                self.negotiated = false;
                return Some(failure(
                    id,
                    -32602,
                    "Unsupported protocol version",
                    Some(json!({
                        "requested": requested,
                        "supported": [TOOLING_PROTOCOL_VERSION],
                    })),
                ));
            }
            self.negotiated = true;
            return Some(RpcResponse::Success {
                jsonrpc: JSON_RPC_VERSION.to_owned(),
                id,
                result: self.handshake_value(),
            });
        }
        if !self.methods.contains(&request.method) {
            return Some(failure(id, -32601, "Method not found", None));
        }
        if !self.negotiated {
            return Some(failure(
                id,
                -32002,
                "OpenSDK tooling handshake is incomplete",
                None,
            ));
        }
        Some(RpcResponse::Success {
            jsonrpc: JSON_RPC_VERSION.to_owned(),
            id,
            result: json!({
                "contract": "dev.pliegors.diagnostics/v1",
                "diagnostics": [],
            }),
        })
    }

    pub fn handshake_value(&self) -> Value {
        json!({
            "protocolVersion": TOOLING_PROTOCOL_VERSION,
            "hostVersion": self.host_version,
            "methods": self.methods,
            "features": self.features,
        })
    }
}

#[derive(Clone, Debug)]
pub struct McpHost {
    tooling: RpcHost,
    initialize_accepted: bool,
    initialized: bool,
}

impl McpHost {
    pub fn new(host_version: impl Into<String>) -> Self {
        Self {
            tooling: RpcHost::new(host_version),
            initialize_accepted: false,
            initialized: false,
        }
    }

    pub fn with_feature(mut self, feature: impl Into<String>) -> Self {
        self.tooling = self.tooling.with_feature(feature);
        self
    }

    pub fn handle(&mut self, request: RpcRequest) -> Option<RpcResponse> {
        if request.jsonrpc != JSON_RPC_VERSION {
            return request
                .id
                .map(|id| failure(id, -32600, "Invalid Request", None));
        }
        if request.method == "notifications/initialized" && request.id.is_none() {
            self.initialized = self.initialize_accepted;
            return None;
        }
        let id = request.id?;
        match request.method.as_str() {
            "initialize" => {
                let params = request.params.as_ref().and_then(Value::as_object);
                let requested = params
                    .and_then(|params| params.get("protocolVersion"))
                    .and_then(Value::as_str);
                let capabilities_are_valid = params
                    .and_then(|params| params.get("capabilities"))
                    .is_some_and(Value::is_object);
                let client_is_valid = params
                    .and_then(|params| params.get("clientInfo"))
                    .and_then(Value::as_object)
                    .is_some_and(|client| {
                        ["name", "version"].iter().all(|field| {
                            client
                                .get(*field)
                                .and_then(Value::as_str)
                                .is_some_and(|value| !value.is_empty() && value.len() <= 128)
                        })
                    });
                if !capabilities_are_valid || !client_is_valid {
                    self.initialize_accepted = false;
                    self.initialized = false;
                    return Some(failure(id, -32602, "Malformed initialize request", None));
                }
                if requested != Some(MCP_PROTOCOL_VERSION) {
                    self.initialize_accepted = false;
                    self.initialized = false;
                    return Some(failure(
                        id,
                        -32602,
                        "Unsupported protocol version",
                        Some(json!({
                            "requested": requested,
                            "supported": [MCP_PROTOCOL_VERSION],
                        })),
                    ));
                }
                self.initialize_accepted = true;
                self.initialized = false;
                Some(RpcResponse::Success {
                    jsonrpc: JSON_RPC_VERSION.to_owned(),
                    id,
                    result: json!({
                        "protocolVersion": MCP_PROTOCOL_VERSION,
                        "capabilities": { "tools": { "listChanged": false } },
                        "serverInfo": {
                            "name": "pliegors-opensdk",
                            "title": "PliegoRS OpenSDK tooling host",
                            "version": TOOLING_PROTOCOL_VERSION,
                        },
                        "instructions": "OpenSDK tooling is capability-negotiated and project-local.",
                    }),
                })
            }
            "ping" => Some(RpcResponse::Success {
                jsonrpc: JSON_RPC_VERSION.to_owned(),
                id,
                result: json!({}),
            }),
            _ if !self.initialized => Some(failure(
                id,
                -32002,
                "MCP initialization is incomplete",
                None,
            )),
            "tools/list" => Some(RpcResponse::Success {
                jsonrpc: JSON_RPC_VERSION.to_owned(),
                id,
                result: json!({
                    "tools": [{
                        "name": "pliego_sdk_handshake",
                        "title": "Inspect PliegoRS OpenSDK capabilities",
                        "description": "Returns the negotiated OpenSDK tooling protocol, methods, and features.",
                        "inputSchema": { "type": "object", "additionalProperties": false },
                    }]
                }),
            }),
            "tools/call" => {
                let params = request.params.as_ref().and_then(Value::as_object);
                let name = params
                    .and_then(|params| params.get("name"))
                    .and_then(Value::as_str);
                if name != Some("pliego_sdk_handshake") {
                    return Some(failure(id, -32602, "Unknown tool", None));
                }
                if !params
                    .and_then(|params| params.get("arguments"))
                    .and_then(Value::as_object)
                    .is_some_and(serde_json::Map::is_empty)
                {
                    return Some(failure(
                        id,
                        -32602,
                        "pliego_sdk_handshake requires empty arguments",
                        None,
                    ));
                }
                let handshake = self.tooling.handshake_value();
                Some(RpcResponse::Success {
                    jsonrpc: JSON_RPC_VERSION.to_owned(),
                    id,
                    result: json!({
                        "content": [{
                            "type": "text",
                            "text": serde_json::to_string(&handshake)
                                .expect("handshake JSON is serializable"),
                        }],
                        "structuredContent": handshake,
                        "isError": false,
                    }),
                })
            }
            _ => Some(failure(id, -32601, "Method not found", None)),
        }
    }
}

fn failure(id: Value, code: i64, message: &str, data: Option<Value>) -> RpcResponse {
    RpcResponse::Failure {
        jsonrpc: JSON_RPC_VERSION.to_owned(),
        id,
        error: RpcError {
            code,
            message: message.to_owned(),
            data,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handshake_is_transport_neutral_and_notifications_have_no_response() {
        let mut host = RpcHost::new("0.1.0-preview.1").with_feature("diagnostic-links");
        let premature = host
            .handle(RpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: Some(json!(6)),
                method: "pliego/diagnostics".to_owned(),
                params: None,
            })
            .unwrap();
        let RpcResponse::Failure { error, .. } = premature else {
            panic!("diagnostics succeeded before negotiation")
        };
        assert_eq!(error.code, -32002);
        let mismatch = host
            .handle(RpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: Some(json!(70)),
                method: "pliego/handshake".to_owned(),
                params: Some(json!({ "protocolVersion": "9.0.0" })),
            })
            .unwrap();
        let RpcResponse::Failure { error, .. } = mismatch else {
            panic!("incompatible tooling handshake succeeded")
        };
        assert_eq!(error.code, -32602);
        let response = host
            .handle(RpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: Some(json!(7)),
                method: "pliego/handshake".to_owned(),
                params: Some(json!({ "protocolVersion": TOOLING_PROTOCOL_VERSION })),
            })
            .unwrap();
        let RpcResponse::Success { result, .. } = response else {
            panic!("expected success")
        };
        assert_eq!(result["protocolVersion"], TOOLING_PROTOCOL_VERSION);
        assert_eq!(result["features"][0], "diagnostic-links");
        let diagnostics = host
            .handle(RpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: Some(json!(8)),
                method: "pliego/diagnostics".to_owned(),
                params: None,
            })
            .unwrap();
        let RpcResponse::Success { result, .. } = diagnostics else {
            panic!("expected diagnostics success")
        };
        assert_eq!(result["contract"], "dev.pliegors.diagnostics/v1");
        assert!(
            host.handle(RpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: None,
                method: "pliego/handshake".to_owned(),
                params: None,
            })
            .is_none()
        );

        let null_id: RpcRequest = serde_json::from_value(json!({
            "jsonrpc": "2.0",
            "id": null,
            "method": "pliego/handshake",
            "params": { "protocolVersion": TOOLING_PROTOCOL_VERSION },
        }))
        .unwrap();
        let RpcResponse::Success { id, .. } = host.handle(null_id).unwrap() else {
            panic!("explicit null id was treated as a notification")
        };
        assert!(id.is_null());
        assert!(
            serde_json::from_value::<RpcRequest>(json!({
                "jsonrpc": "2.0",
                "id": {},
                "method": "pliego/handshake",
            }))
            .is_err()
        );
    }

    #[test]
    fn mcp_client_uses_the_same_tooling_handshake_after_strict_initialization() {
        let mut host = McpHost::new("0.1.0-preview.1").with_feature("diagnostic-links");
        assert!(
            host.handle(RpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: None,
                method: "notifications/initialized".to_owned(),
                params: None,
            })
            .is_none()
        );
        let premature = host
            .handle(RpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: Some(json!(0)),
                method: "tools/list".to_owned(),
                params: None,
            })
            .unwrap();
        let RpcResponse::Failure { error, .. } = premature else {
            panic!("premature initialization must fail")
        };
        assert_eq!(error.code, -32002);
        let initialize = host
            .handle(RpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: Some(json!(1)),
                method: "initialize".to_owned(),
                params: Some(json!({
                    "protocolVersion": MCP_PROTOCOL_VERSION,
                    "capabilities": {},
                    "clientInfo": { "name": "fixture", "version": "0.1.0" },
                })),
            })
            .unwrap();
        let RpcResponse::Success { result, .. } = initialize else {
            panic!("initialize failed")
        };
        assert_eq!(result["protocolVersion"], MCP_PROTOCOL_VERSION);
        assert!(
            host.handle(RpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: None,
                method: "notifications/initialized".to_owned(),
                params: None,
            })
            .is_none()
        );
        let call = host
            .handle(RpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: Some(json!(2)),
                method: "tools/call".to_owned(),
                params: Some(json!({ "name": "pliego_sdk_handshake", "arguments": {} })),
            })
            .unwrap();
        let RpcResponse::Success { result, .. } = call else {
            panic!("tool call failed")
        };
        assert_eq!(
            result["structuredContent"]["protocolVersion"],
            TOOLING_PROTOCOL_VERSION
        );
    }
}
