//! Model plane: the unified logical-model catalog, endpoint seeding within
//! disk budgets, and the runtime router. OS-native backends (MLX, ONNX,
//! llama.cpp) live behind traits — the router never imports them directly.
//!
//! First-class model modules compile in via Cargo features. The pipeline
//! calls [`available_modules`] for runtime discovery of what was compiled
//! into this binary.

pub use fabric_types::modules::{ModuleInfo, ModuleTask};

/// Returns the first-class model modules compiled into this binary.
///
/// Each entry is gated by the corresponding Cargo feature; stripping a
/// feature (e.g. for regulated environments) removes the module from this
/// list and from the compiled artifact.
#[allow(clippy::vec_init_then_push)]
pub fn available_modules() -> Vec<ModuleInfo> {
    let mut modules = Vec::new();

    #[cfg(feature = "safety-nemotron-cs")]
    modules.push(ModuleInfo {
        name: "nemotron_cs",
        task: ModuleTask::Safety,
        model_family: "nvidia/nemotron-3.5-content-safety",
        default_endpoint: None,
    });

    #[cfg(feature = "safety-llama-guard")]
    modules.push(ModuleInfo {
        name: "llama_guard",
        task: ModuleTask::Safety,
        model_family: "meta-llama/llama-guard",
        default_endpoint: None,
    });

    #[cfg(feature = "safety-granite-guardian")]
    modules.push(ModuleInfo {
        name: "granite_guardian",
        task: ModuleTask::Safety,
        model_family: "ibm-granite/granite-guardian",
        default_endpoint: None,
    });

    #[cfg(feature = "safety-shield-gemma")]
    modules.push(ModuleInfo {
        name: "shield_gemma",
        task: ModuleTask::Safety,
        model_family: "google/shieldgemma",
        default_endpoint: None,
    });

    #[cfg(feature = "decoder-nemotron")]
    modules.push(ModuleInfo {
        name: "constrained_decoder",
        task: ModuleTask::Decoder,
        model_family: "nvidia/nemotron-3-nano-30b-a3b",
        default_endpoint: None,
    });

    #[cfg(feature = "mediator-nemotron")]
    modules.push(ModuleInfo {
        name: "constrained_mediator",
        task: ModuleTask::Mediator,
        model_family: "nvidia/nemotron-3-nano-30b-a3b",
        default_endpoint: None,
    });

    modules
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_modules_are_present() {
        let names: Vec<&str> = available_modules().iter().map(|m| m.name).collect();

        #[cfg(feature = "safety-nemotron-cs")]
        assert!(names.contains(&"nemotron_cs"));
        #[cfg(feature = "safety-llama-guard")]
        assert!(names.contains(&"llama_guard"));
        #[cfg(feature = "safety-granite-guardian")]
        assert!(names.contains(&"granite_guardian"));
        #[cfg(feature = "safety-shield-gemma")]
        assert!(names.contains(&"shield_gemma"));
        #[cfg(feature = "decoder-nemotron")]
        assert!(names.contains(&"constrained_decoder"));
        #[cfg(feature = "mediator-nemotron")]
        assert!(names.contains(&"constrained_mediator"));
    }

    #[test]
    fn module_tasks_cover_scoped_tasks() {
        let modules = available_modules();
        assert!(modules.iter().any(|m| m.task == ModuleTask::Safety));
        assert!(modules.iter().any(|m| m.task == ModuleTask::Decoder));
        assert!(modules.iter().any(|m| m.task == ModuleTask::Mediator));
    }
}
