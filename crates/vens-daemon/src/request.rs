use roxmltree::{Document, Node, ParsingOptions};
use vens_escl::{
    bridge::Request,
    route::{Route, route},
    settings::{JobSettings, Requested, Unsupported, accept},
};

use crate::http::HttpRequest;

const PWG: &str = "http://www.pwg.org/schemas/2010/12/sm";
const SCAN: &str = "http://schemas.hp.com/imaging/escl/2011/05/03";
/// Bounds the parse of a `ScanSettings` body, which clients send with a few dozen nodes.
const NODES_LIMIT: u32 = 256;
const XML_TYPES: [&str; 2] = ["text/xml", "application/xml"];

/// A job request whose body isn't a `ScanSettings` document.
struct Unreadable;

fn value<'a>(root: Node<'a, '_>, namespace: &str, name: &str) -> Option<&'a str> {
    root.children()
        .find(|child| child.has_tag_name((namespace, name)))
        .and_then(|child| child.text())
        .map(str::trim)
}

fn token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte)
}

fn after_parameter_value(value: &str) -> Option<&str> {
    let Some(quoted) = value.strip_prefix('"') else {
        let length = value.bytes().take_while(|&byte| token_byte(byte)).count();
        return if length == 0 {
            None
        } else {
            value.get(length..)
        };
    };
    let mut escaped = false;
    for (offset, byte) in quoted.bytes().enumerate() {
        if (byte < b' ' && byte != b'\t') || byte == 0x7f {
            return None;
        }
        match (escaped, byte) {
            (true, _) => escaped = false,
            (false, b'\\') => escaped = true,
            (false, b'"') => return quoted.get(offset.saturating_add(1)..),
            (false, _) => {}
        }
    }
    None
}

fn xml_media_type(content_type: Option<&str>) -> bool {
    let Some(media) = content_type.map(|value| value.trim_matches([' ', '\t'])) else {
        return false;
    };
    let (essence, mut remaining) = media.split_at(media.find(';').unwrap_or(media.len()));
    if !XML_TYPES.iter().any(|xml| {
        essence
            .trim_end_matches([' ', '\t'])
            .eq_ignore_ascii_case(xml)
    }) {
        return false;
    }
    while let Some(parameter) = remaining.strip_prefix(';') {
        let parameter = parameter.trim_start_matches([' ', '\t']);
        if parameter.is_empty() || parameter.starts_with(';') {
            remaining = parameter;
            continue;
        }
        let Some((name, value)) = parameter.split_once('=') else {
            return false;
        };
        if name.is_empty() || !name.bytes().all(token_byte) {
            return false;
        }
        let Some(rest) = after_parameter_value(value) else {
            return false;
        };
        remaining = rest.trim_start_matches([' ', '\t']);
    }
    remaining.is_empty()
}

fn scan_settings(request: &HttpRequest) -> Result<Result<JobSettings, Unsupported>, Unreadable> {
    if !xml_media_type(request.content_type.as_deref()) {
        return Err(Unreadable);
    }
    let text = str::from_utf8(&request.body).map_err(|_| Unreadable)?;
    let options = ParsingOptions {
        nodes_limit: NODES_LIMIT,
        ..ParsingOptions::default()
    };
    let document = Document::parse_with_options(text, options).map_err(|_| Unreadable)?;
    let root = document.root_element();
    if !root.has_tag_name((SCAN, "ScanSettings")) {
        return Err(Unreadable);
    }
    Ok(accept(&Requested {
        input_source: value(root, PWG, "InputSource"),
        duplex: value(root, SCAN, "Duplex"),
        color_mode: value(root, SCAN, "ColorMode"),
        document_format: value(root, PWG, "DocumentFormat"),
        document_format_ext: value(root, SCAN, "DocumentFormatExt"),
        x_resolution: value(root, SCAN, "XResolution"),
        y_resolution: value(root, SCAN, "YResolution"),
    }))
}

/// Names the bridge request for a read HTTP request, with the settings of a job request.
///
/// A job request must declare an XML media type.
#[must_use]
pub fn escl_request(request: &HttpRequest) -> Request {
    match route(&request.method, &request.target) {
        Ok(Route::Capabilities) => Request::Capabilities,
        Ok(Route::Status) => Request::Status,
        Ok(Route::CreateJob) => {
            scan_settings(request).map_or(Request::Malformed, Request::CreateJob)
        }
        Ok(Route::NextDocument(id)) => Request::NextDocument(id),
        Ok(Route::DeleteJob(id)) => Request::DeleteJob(id),
        Err(unrouted) => Request::Unrouted(unrouted),
    }
}
