#![cfg_attr(not(test), allow(dead_code))]

mod aggregate;
mod history;
mod model;

#[cfg(test)]
mod history_tests;
#[cfg(test)]
mod model_tests;
