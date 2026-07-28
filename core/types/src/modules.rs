//! Model module discovery types. Hand-written (not generated): describes
//! the first-class model modules compiled into a binary via Cargo features.

/// Describes a model module for discovery and documentation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleInfo {
    pub name: &'static str,
    pub task: ModuleTask,
    pub model_family: &'static str,
    pub default_endpoint: Option<&'static str>,
}

/// The scoped inference task a module serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleTask {
    Decoder,
    Mediator,
    Safety,
}

impl ModuleTask {
    pub fn as_str(&self) -> &'static str {
        match self {
            ModuleTask::Decoder => "decoder",
            ModuleTask::Mediator => "mediator",
            ModuleTask::Safety => "safety",
        }
    }
}
