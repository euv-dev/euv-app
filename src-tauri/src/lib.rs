//! EUV App
//!
//! A Tauri-based application with offline cache and remote resource synchronization.

#[macro_use]
mod macros;

mod bridge;
mod cache;

pub use {bridge::*, cache::*};

pub(crate) use std::{
    borrow::Cow,
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
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
