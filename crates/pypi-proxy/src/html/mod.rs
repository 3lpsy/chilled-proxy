//! PEP 503 HTML simple-index parsing into the PEP 691 JSON model.
//!
//! An HTML-only upstream is normalized at ingest — the whole pipeline speaks
//! PEP 691 — so an HTML index is gated exactly like a JSON one.

pub(crate) mod convert;
pub(crate) mod entity;
pub(crate) mod scan;

pub(crate) use convert::{has_upload_times, is_html_simple, parse_simple_html};
