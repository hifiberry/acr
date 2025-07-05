// Module declaration for librespot player implementation
mod event_common;
mod event_pipe_reader;
mod librespot;

// Re-export for easier access from parent module
pub use librespot::LibrespotPlayerController;
pub use event_common::{EventCallback, LibrespotEventProcessor};