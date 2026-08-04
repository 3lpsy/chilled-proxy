//! PEP 503 HTML simple-index parsing into the PEP 691 JSON model.
//!
//! The proxy's whole pipeline — age filter, URL rewriting, validators, render —
//! speaks the PEP 691 document shape. An HTML-only upstream is normalized here
//! at ingest rather than handled as a second format downstream, so an index
//! served as HTML is gated exactly like one served as JSON.

pub(crate) mod convert;
pub(crate) mod entity;
pub(crate) mod scan;

pub(crate) use convert::{has_upload_times, is_html_simple, parse_simple_html};
