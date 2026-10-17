//! Linear integration (personal API key, GraphQL). See docs/MEETING_NOTES_ARCH.md.
//!
//! - [`client`]: async GraphQL client ([`client::LinearClient`]).
//! - [`draft`]: pure action item -> [`crate::meetings::types::TicketDraft`].
//! - [`create`]: idempotent creation of approved tickets.
//! - [`service`]: state behind the `MeetingsManager` Linear section.

pub mod cache;
pub mod client;
pub mod create;
pub mod draft;
pub mod service;
#[cfg(test)]
mod tests;

pub use client::{CreatedIssue, LinearClient, LinearError, Viewer};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Team {
    pub id: String,
    pub key: String,
    pub name: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub id: String,
    pub name: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct User {
    pub id: String,
    pub name: String,
    pub display_name: String,
    pub email: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Label {
    pub id: String,
    pub name: String,
}
