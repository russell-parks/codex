mod extension;
mod state;

pub use extension::install;

#[cfg(test)]
#[path = "extension_tests.rs"]
mod extension_tests;
