pub mod plugin;
pub mod plugin_factory;
pub mod plugin_manager;
pub mod event_filters;

// Re-export commonly used items
pub use plugin::Plugin;
pub use event_filters::EventFilter;