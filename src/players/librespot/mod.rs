// Module declaration for librespot player implementation
mod event_common;
mod event_pipe_reader;
mod event_api_processor;
mod librespot;

// Re-export for easier access from parent module
// Removed unused import: event_pipe_reader::EventPipeReader
pub use librespot::LibrespotPlayerController;
pub use event_api_processor::{EventApiProcessor, register_processor, unregister_processor, librespot_event_update};
pub use event_common::{EventCallback, LibrespotEventProcessor};