pub mod admin;
pub mod auth;
pub mod employee;
pub mod eod;
pub mod flash;
pub mod health;
pub mod leave;
pub mod manager;
pub mod metrics;
pub mod notifications;
pub mod profile;
pub mod render;
pub mod requirements;

pub use render::{HtmlPage, PageOrRedirect};
