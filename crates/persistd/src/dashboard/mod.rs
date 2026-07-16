#![cfg_attr(not(test), allow(dead_code))]

mod aggregate;
mod format;
mod history;
mod model;
mod proc_source;
mod procfs;
mod storage;
mod storage_reader;
mod storage_security;

#[cfg(test)]
mod format_tests;

#[cfg(test)]
mod history_tests;
#[cfg(test)]
mod model_tests;
#[cfg(test)]
mod procfs_tests;
#[cfg(test)]
mod storage_tests;
