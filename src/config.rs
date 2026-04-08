use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub title: String,
    pub welcome: Welcome,
    pub steps: Vec<StepDefinition>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Welcome {
    pub ask: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StepDefinition {
    pub title: String,
    pub description: String,
    #[serde(default)]
    pub parallel: bool,
    pub run: Vec<String>,
}

pub fn parse_config(json: &str) -> Result<AppConfig, String> {
    serde_json::from_str(json).map_err(|e| format!("Ungültige Konfiguration: {e}"))
}
