//! No-clobber file installation for the public conversion API.

mod install;
mod stage;

pub(crate) use install::{create_new, create_pair};

#[cfg(test)]
mod tests;
