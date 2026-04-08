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
    pub run: Vec<RunEntry>,
}

/// A single command entry in `run[]` — either a plain string (legacy) or a structured object.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum RunEntry {
    Simple(String),
    Detailed(RunObject),
}

#[derive(Debug, Clone, Deserialize)]
pub struct RunObject {
    pub task: String,
    #[serde(default)]
    pub flags: Vec<String>,
    #[serde(default)]
    pub params: Vec<String>,
    #[serde(default)]
    pub onerror: OnError,
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum OnError {
    /// Abort the step (and all subsequent steps) on failure. Default.
    #[default]
    Stop,
    /// Log the error as a warning and continue.
    ContinueMessage,
    /// Silently ignore the error and continue.
    ContinueSilent,
}

impl RunEntry {
    /// Returns the command string to pass to `parse_command`.
    pub fn command_string(&self) -> String {
        match self {
            RunEntry::Simple(s)    => s.clone(),
            RunEntry::Detailed(obj) => obj.to_command_string(),
        }
    }

    pub fn onerror(&self) -> OnError {
        match self {
            RunEntry::Simple(_)    => OnError::Stop,
            RunEntry::Detailed(obj) => obj.onerror.clone(),
        }
    }
}

impl RunObject {
    fn to_command_string(&self) -> String {
        let verb = if self.flags.is_empty() {
            self.task.clone()
        } else {
            format!("{}:{}", self.task, self.flags.join(":"))
        };
        if self.params.is_empty() {
            verb
        } else {
            let param_str = self.params.iter()
                .map(|p| {
                    if p.contains(' ') || p.contains('"') {
                        format!("\"{}\"", p.replace('"', "\\\""))
                    } else {
                        p.clone()
                    }
                })
                .collect::<Vec<_>>()
                .join(" ");
            format!("{verb} {param_str}")
        }
    }
}

pub fn parse_config(json: &str) -> Result<AppConfig, String> {
    serde_json::from_str(json).map_err(|e| format!("Ungültige Konfiguration: {e}"))
}
