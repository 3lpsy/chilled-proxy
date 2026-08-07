//! `maven-metadata.xml` age-gating filter (quick-xml streaming rewrite):
//! drops `<version>` entries newer than the cutoff (or with no recorded age —
//! fail-closed) and repoints `<latest>`/`<release>`. Ages come from POM
//! probes, never the bypassable `<lastUpdated>` field.

mod rewrite;
mod scan;

#[cfg(test)]
mod tests;

pub(crate) use rewrite::filter_metadata;
pub(crate) use scan::list_versions;
