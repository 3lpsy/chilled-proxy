//! Registry data types and their on-disk cache path rules.

pub(crate) mod crate_info;
pub(crate) mod file;
pub(crate) mod index_entry;

pub(crate) use crate_info::CrateInfo;
pub(crate) use file::{
    cache_fetch_crate, cache_fetch_index_entry, cache_store_crate, cache_store_index_entry,
    cache_try_find_index_entry,
};
pub(crate) use index_entry::IndexEntry;
