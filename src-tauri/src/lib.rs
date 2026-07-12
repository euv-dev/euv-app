//! EUV App
//!
//! A Tauri-based application with offline cache and remote resource synchronization.

mod bridge;
mod cache;
mod log;

pub use cache::*;

pub(crate) use bridge::*;

pub(crate) use std::{
    borrow::Cow,
    collections::HashSet,
    path::{Path, PathBuf},
    sync::OnceLock,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub(crate) use {
    lombok_macros::*,
    reqwest::Client,
    serde::{Serialize, Serializer},
    tauri::{
        App, AppHandle, Builder, Manager, RunEvent, UriSchemeResponder, Webview,
        async_runtime::spawn,
        generate_context, generate_handler,
        webview::{PageLoadEvent, PageLoadPayload},
    },
    tokio::fs::{
        create_dir_all, metadata, read, read_dir, read_to_string, remove_dir_all, rename, write,
    },
};
