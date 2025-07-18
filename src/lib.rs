#![cfg_attr(not(test), warn(unused_crate_dependencies))]
use dotenvy as _;

pub mod any_or_value;
pub mod auth;
pub mod cli;
pub mod client;
pub mod fanout;
pub mod metrics;
pub mod proxy;
pub mod rpc;
pub mod validation;
