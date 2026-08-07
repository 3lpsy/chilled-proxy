//! Cooldown-aware ETag marker scheme: a filtered/rewritten body must not be
//! served under the upstream's strong ETag, so proxies issue a weak marked
//! variant and strip the marker back off when a client revalidates. Marker
//! grammar (inside the quotes): `<inner>[.cd<secs>-<bucket>|.rw][.<fmt>]`.

#[cfg(test)]
mod tests;

mod marker;
mod validators;

pub use marker::{
    etag_format, etag_inner, etag_marker, filtered_etag, format_etag, rewrite_etag, unmark_etag,
    Marker,
};
pub use validators::cooldown_validators;
