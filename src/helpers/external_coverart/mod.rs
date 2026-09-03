//! Cover art from HTTP endpoints named in the configuration.
//!
//! Unlike the providers in `coverart_providers`, these are not written
//! against a known service: an endpoint speaks a fixed JSON contract
//! documented in `doc/external-coverart.md`, and anything unusual about the
//! service behind it is that service's problem. They are assumed slow -- the
//! first one is an LLM-backed lookup taking 20-40 seconds -- so they are
//! never on a request path unless a caller opts in.

pub mod config;
