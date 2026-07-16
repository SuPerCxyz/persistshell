#![cfg_attr(not(test), allow(dead_code))]

mod aggregate;
mod history;
mod model;
mod proc_source;
mod procfs;

#[cfg(test)]
mod history_tests;
#[cfg(test)]
mod model_tests;
#[cfg(test)]
mod procfs_tests;
