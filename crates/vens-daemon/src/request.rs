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
/// The media types that a browser form sends without asking the server first.
const FORM_TYPES: [&str; 3] = [
    "text/plain",
    "application/x-www-form-urlencoded",
    "multipart/form-data",
];

/// A job request whose body isn't a `ScanSettings` document.
struct Unreadable;

fn value<'a>(root: Node<'a, '_>, namespace: &str, name: &str) -> Option<&'a str> {
    root.children()
        .find(|child| child.has_tag_name((namespace, name)))
        .and_then(|child| child.text())
        .map(str::trim)
}

/// Tells a body that a web page could have sent through a browser form from a scan client's body.
fn from_a_form(content_type: Option<&str>) -> bool {
    content_type.is_some_and(|media| {
        let essence = media.split_once(';').map_or(media, |(essence, _)| essence);
        FORM_TYPES
            .iter()
            .any(|form| essence.trim().eq_ignore_ascii_case(form))
    })
}

fn scan_settings(request: &HttpRequest) -> Result<Result<JobSettings, Unsupported>, Unreadable> {
    if from_a_form(request.content_type.as_deref()) {
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
/// A job request with a media type that a browser form sends is malformed, so a web page can't start a scan.
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
