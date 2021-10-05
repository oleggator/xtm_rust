use serde::{Deserialize, Serialize};
use tokio::runtime;

#[derive(Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ModuleConfig {
    pub buffer: usize,
    pub runtime: RuntimeConfig
}

impl Default for ModuleConfig {
    fn default() -> Self {
        ModuleConfig{
            buffer: 128,
            runtime: RuntimeConfig::default(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum RuntimeConfig {
    #[serde(rename(deserialize = "cur_thread"))]
    CurrentThread,

    #[serde(rename(deserialize = "multi_thread"))]
    MultiThread { thread_count: Option<usize> },
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self::MultiThread { thread_count: None }
    }
}

impl From<RuntimeConfig> for runtime::Builder {
    fn from(runtime_options: RuntimeConfig) -> Self {
        match runtime_options {
            RuntimeConfig::CurrentThread => runtime::Builder::new_current_thread(),
            RuntimeConfig::MultiThread { thread_count } => {
                let mut builder = runtime::Builder::new_multi_thread();
                if let Some(threads) = thread_count {
                    builder.worker_threads(threads);
                }
                builder
            }
        }
    }
}
