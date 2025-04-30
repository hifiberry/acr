// Module declaration for librespot player implementation
mod event_pipe_reader;
mod librespot;

// Re-export for easier access from parent module
// Removed unused import: event_pipe_reader::EventPipeReader
pub use librespot::LibrespotPlayerController;