//! Tool plane: device-sticky tool registry, dispatch, and container runtime.
//! Every tool call goes through the policy gate, then routes to the appropriate
//! tool implementation. Tools are never leased — they stay on the device that
//! owns the hands.

pub mod container;
pub mod dispatch;
pub mod terminal;

pub use container::{
    ContainerError, ContainerId, ContainerInfo, ContainerRuntime, ContainerSpec, ExecOutput,
    ExecSpec, ImageRef, ListFilter, NetworkPolicy, RegistryAuth,
};
pub use dispatch::{Tool, ToolDispatcher, ToolError, ToolRegistry};
pub use terminal::{TerminalConfig, TerminalTool};
