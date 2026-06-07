//! EUV App
//!
//! A Tauri-based application with offline cache and remote resource synchronization.

mod bridge;
mod cache;
mod log;

pub use cache::*;

pub(crate) use bridge::*;

#[cfg(debug_assertions)]
pub(crate) use std::sync::OnceLock;
pub(crate) use std::{
    borrow::Cow,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub(crate) use {
    reqwest::{Client, redirect::Policy},
    serde::Serialize,
    tauri::{
        App, AppHandle, Builder, Manager, RunEvent, async_runtime::spawn, generate_context,
        generate_handler,
    },
    tokio::fs::{create_dir_all, read_dir, read_to_string, remove_dir_all, rename, write},
};
