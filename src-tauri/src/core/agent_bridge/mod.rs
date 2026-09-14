//! Sending prompts into coding-agent conversations.
//!
//! The counterpart of `agent_sessions`, which only reads. Everything here acts on
//! an agent app: it continues conversations in processes SOURCE owns, and hands
//! them back when it's done.

pub mod brief;
pub mod bridge;
pub mod claude_cli;
pub mod claude_driver;
pub mod driver_events;
pub mod handoff;
pub mod stream_protocol;

#[cfg(test)]
mod bridge_tests;
#[cfg(test)]
mod driver_tests;
#[cfg(test)]
mod handoff_tests;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;

pub use bridge::{AgentBridge, BridgeEvent, SendError};
pub use driver_events::DriverEvent;
pub use handoff::HandoffError;
