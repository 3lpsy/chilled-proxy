//! Version-list extraction from pristine metadata XML.

use quick_xml::events::Event;
use quick_xml::Reader;

/// Lists the `<version>` texts inside `<versions>`, in document order.
pub(crate) fn list_versions(xml: &[u8]) -> Result<Vec<String>, String> {
    let mut reader = Reader::from_reader(xml);
    let mut buf = Vec::new();
    let mut stack: Vec<Vec<u8>> = Vec::new();
    let mut versions = Vec::new();

    loop {
        match reader
            .read_event_into(&mut buf)
            .map_err(|e| e.to_string())?
        {
            Event::Start(e) => stack.push(e.local_name().as_ref().to_vec()),
            Event::End(_) => {
                stack.pop();
            }
            Event::Text(t) => {
                let n = stack.len();
                if n >= 2 && stack[n - 1] == b"version" && stack[n - 2] == b"versions" {
                    let text = t.unescape().map_err(|e| e.to_string())?;
                    let text = text.trim();
                    if !text.is_empty() {
                        versions.push(text.to_owned());
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(versions)
}
