#![deny(clippy::all)]

use std::sync::mpsc;
use std::thread;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FeatureId(pub String);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum KernelError {
    OpenCascadeFailure(String),
    InvalidGeometry(String),
    FeatureNotFound(FeatureId),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SketchSupport {
    PlaneXY,
    PlaneXZ,
    PlaneYZ,
    Face(FeatureId),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SketchPrimitive {
    Rectangle { width: f64, height: f64 },
    Circle { radius: f64 },
    Line { start: (f64, f64), end: (f64, f64) },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum BooleanMode {
    Add,
    Subtract,
    Intersect,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HoleSpec {
    Simple { diameter: f64, depth: f64 },
    Counterbore { diameter: f64, cb_diameter: f64, cb_depth: f64, depth: f64 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]

pub enum CadToolCall {
    SetParameter { name: String, value: f64 },
    CreateSketch { support: SketchSupport, primitives: Vec<SketchPrimitive> },
    Extrude { sketch_id: FeatureId, distance: f64, mode: BooleanMode },
    Fillet { feature_id: FeatureId, radius: f64 },
    Chamfer { feature_id: FeatureId, distance: f64 },
    CreateHole { location: (f64, f64, f64), spec: HoleSpec },
    Shell { feature_id: FeatureId, thickness: f64 },
}

pub trait CadDocumentBridge {
    fn execute_tool_call(&mut self, call: &CadToolCall) -> Result<FeatureId, KernelError>;
}

pub fn build_cad_tools_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "tool_calls": {
                "type": "array",
                "items": {
                    "type": "object"
                }
            }
        }
    })
}

pub fn execute_cad_pipeline<B: CadDocumentBridge>(
    bridge: &mut B,
    calls: &[CadToolCall],
) -> Result<Vec<FeatureId>, KernelError> {
    let mut created_features = Vec::new();
    for call in calls {
        let feature_id = bridge.execute_tool_call(call)?;
        created_features.push(feature_id);
    }
    Ok(created_features)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiEndpointConfig {
    pub api_url: String,
    pub api_key: String,
    pub model: String,
}

pub fn spawn_llm_request(
    config: AiEndpointConfig,
    prompt: String,
) -> mpsc::Receiver<Result<String, String>> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let body = serde_json::json!({
            "model": config.model,
            "messages": [{"role": "user", "content": prompt}]
        });
        let response = ureq::post(&config.api_url)
            .set("Authorization", &format!("Bearer {}", config.api_key))
            .set("Content-Type", "application/json")
            .send_json(body);

        match response {
            Ok(res) => {
                let text = res.into_string().unwrap_or_default();
                let _ = tx.send(Ok(text));
            }
            Err(e) => {
                let _ = tx.send(Err(e.to_string()));
            }
        }
    });
    rx
}

pub struct AiAssistantPanel {
    pub config: AiEndpointConfig,
    pub auto_fix_attempts_left: usize,
}

impl AiAssistantPanel {
    pub fn new(config: AiEndpointConfig) -> Self {
        Self {
            config,
            auto_fix_attempts_left: 3,
        }
    }

    pub fn process_with_autofix<B: CadDocumentBridge>(
        &mut self,
        bridge: &mut B,
        calls: &[CadToolCall],
    ) -> Result<Vec<FeatureId>, KernelError> {
        let mut attempts = self.auto_fix_attempts_left;
        loop {
            match execute_cad_pipeline(bridge, calls) {
                Ok(features) => return Ok(features),
                Err(err) => {
                    if attempts == 0 {
                        return Err(err);
                    }
                    attempts -= 1;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DummyBridge {
        counter: usize,
    }

    impl CadDocumentBridge for DummyBridge {
        fn execute_tool_call(&mut self, _call: &CadToolCall) -> Result<FeatureId, KernelError> {
            self.counter += 1;
            Ok(FeatureId(format!("feat_{}", self.counter)))
        }
    }

    #[test]
    fn parses_and_executes_parametric_bracket_plan() {
        let mut bridge = DummyBridge { counter: 0 };
        let plan = vec![
            CadToolCall::SetParameter {
                name: "bracket_width".to_string(),
                value: 50.0,
            },
            CadToolCall::CreateSketch {
                support: SketchSupport::PlaneXY,
                primitives: vec![SketchPrimitive::Rectangle {
                    width: 50.0,
                    height: 100.0,
                }],
            },
            CadToolCall::Extrude {
                sketch_id: FeatureId("feat_2".to_string()),
                distance: 10.0,
                mode: BooleanMode::Add,
            },
            CadToolCall::Fillet {
                feature_id: FeatureId("feat_3".to_string()),
                radius: 2.0,
            },
            CadToolCall::Chamfer {
                feature_id: FeatureId("feat_3".to_string()),
                distance: 1.0,
            },
            CadToolCall::CreateHole {
                location: (25.0, 50.0, 0.0),
                spec: HoleSpec::Simple {
                    diameter: 8.0,
                    depth: 10.0,
                },
            },
            CadToolCall::Shell {
                feature_id: FeatureId("feat_3".to_string()),
                thickness: 1.5,
            },
        ];

        let json = serde_json::to_string(&plan).expect("Failed to serialize plan");
        let parsed: Vec<CadToolCall> = serde_json::from_str(&json).expect("Failed to parse plan");

        let features = execute_cad_pipeline(&mut bridge, &parsed).expect("Pipeline execution failed");
        assert_eq!(features.len(), 7);
    }
}
