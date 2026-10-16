use geospec_engine_native_core::{
    canonicalize as core_canonicalize, Engine as CoreEngine, EngineConfig,
    ProtocolError as CoreProtocolError,
};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(js_name = ProtocolError)]
pub struct ProtocolError {
    code: String,
    message: String,
    capacity_code: Option<String>,
    capacity_details: Option<String>,
}

#[wasm_bindgen(js_class = ProtocolError)]
impl ProtocolError {
    #[wasm_bindgen(getter)]
    pub fn code(&self) -> String {
        self.code.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn message(&self) -> String {
        self.message.clone()
    }

    /// `GEOSPEC_RESOURCE_LIMIT` or `GEOSPEC_ANALYSIS_LIMIT` for a capacity refusal.
    #[wasm_bindgen(getter, js_name = capacityCode)]
    pub fn capacity_code(&self) -> Option<String> {
        self.capacity_code.clone()
    }

    /// Canonical JSON `{limit, limitClass, required, unit}` for a capacity refusal.
    #[wasm_bindgen(getter, js_name = capacityDetails)]
    pub fn capacity_details(&self) -> Option<String> {
        self.capacity_details.clone()
    }
}

impl From<CoreProtocolError> for ProtocolError {
    fn from(error: CoreProtocolError) -> Self {
        Self {
            code: error.code().to_owned(),
            message: error.to_string(),
            capacity_code: error.capacity_code().map(str::to_owned),
            capacity_details: error
                .capacity_details()
                .and_then(|details| String::from_utf8(details).ok()),
        }
    }
}

fn fail(error: CoreProtocolError) -> JsValue {
    ProtocolError::from(error).into()
}

#[wasm_bindgen]
pub struct Engine {
    inner: CoreEngine,
}

impl Default for Engine {
    fn default() -> Self {
        Self {
            inner: CoreEngine::new(),
        }
    }
}

#[wasm_bindgen]
impl Engine {
    /// Create an engine; `limitsJson` optionally holds strict JSON limits with
    /// camelCase names, and omitted limits keep the entry profile.
    #[wasm_bindgen(constructor)]
    pub fn new(
        execution_permits: Option<f64>,
        limits_json: Option<String>,
    ) -> Result<Self, JsValue> {
        let config = EngineConfig::entry()
            .with_execution_permits(execution_permits.unwrap_or(1.0))
            .map_err(|message| {
                JsValue::from(ProtocolError {
                    code: "invalid-request".into(),
                    message: message.into(),
                    capacity_code: None,
                    capacity_details: None,
                })
            })?;
        let config = match limits_json {
            None => config,
            Some(limits) => config.with_limits_json(limits.as_bytes()).map_err(fail)?,
        };
        Ok(Self {
            inner: CoreEngine::with_config(config),
        })
    }

    #[wasm_bindgen(js_name = ingestMesh)]
    pub fn ingest_mesh(&mut self, request: &[u8], mesh: &[u8]) -> Result<Vec<u8>, JsValue> {
        self.inner.ingest_mesh(request, mesh).map_err(fail)
    }

    #[wasm_bindgen(js_name = processRequest)]
    pub fn process_request(&self, request: &[u8]) -> Result<Vec<u8>, JsValue> {
        self.inner.process_request(request).map_err(fail)
    }

    #[wasm_bindgen(js_name = canonicalPlan)]
    pub fn canonical_plan(&self, request: &[u8]) -> Result<Vec<u8>, JsValue> {
        self.inner.canonical_plan(request).map_err(fail)
    }

    #[wasm_bindgen(js_name = evaluatePlan)]
    pub fn evaluate_plan(&self, plan: &[u8]) -> Result<Vec<u8>, JsValue> {
        self.inner.evaluate_plan(plan).map_err(fail)
    }

    #[wasm_bindgen(js_name = evaluateClaim)]
    pub fn evaluate_claim(&self, request: &[u8]) -> Result<Vec<u8>, JsValue> {
        self.inner.evaluate_claim(request).map_err(fail)
    }
}

#[wasm_bindgen]
pub fn canonicalize(input: &[u8]) -> Result<Vec<u8>, JsValue> {
    core_canonicalize(input).map_err(fail)
}
