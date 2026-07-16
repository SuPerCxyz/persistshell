#![cfg_attr(not(test), allow(dead_code))]

mod aggregate;
mod format;
mod history;
mod ipc;
mod ipc_disk;
mod model;
mod proc_source;
mod procfs;
mod storage;
mod storage_reader;
mod storage_security;
mod worker;
mod writer;

pub(crate) use ipc::{unavailable_summary, unavailable_trend, DashboardService};
pub(crate) use procfs::SessionRoot;
pub(crate) use worker::{DashboardRuntime, SampleRequest, SAMPLE_INTERVAL};

#[cfg(test)]
mod format_tests;

#[cfg(test)]
mod history_tests;
#[cfg(test)]
mod ipc_tests;
#[cfg(test)]
mod model_tests;
#[cfg(test)]
mod procfs_tests;
#[cfg(test)]
mod storage_tests;
#[cfg(test)]
mod worker_tests;
#[cfg(test)]
mod writer_tests;
