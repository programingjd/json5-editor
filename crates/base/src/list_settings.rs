use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ListSettings {
    pub active_highlight: bool,
}

impl Default for ListSettings {
    fn default() -> Self {
        Self {
            active_highlight: true,
        }
    }
}
