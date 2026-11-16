use roxmltree::{Document, Node};
use vens_escl::{
    bridge::Request,
    route::{Route, route},
    settings::{JobSettings, Requested, Unsupported, accept},
};

use crate::http::HttpRequest;

const PWG: &str = "http://www.pwg.org/schemas/2010/12/sm";
const SCAN: &str = "http://schemas.hp.com/imaging/escl/2011/05/03";

/// A job request whose body isn't a `ScanSettings` document.
struct Unreadable;

fn value<'a>(root: Node<'a, '_>, namespace: &str, name: &str) -> Option<&'a str> {
    root.children()
        .find(|child| child.has_tag_name((namespace, name)))
        .and_then(|child| child.text())
        .map(str::trim)
}

fn scan_settings(body: &[u8]) -> Result<Result<JobSettings, Unsupported>, Unreadable> {
    let text = str::from_utf8(body).map_err(|_| Unreadable)?;
    let document = Document::parse(text).map_err(|_| Unreadable)?;
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
#[must_use]
pub fn escl_request(request: &HttpRequest) -> Request {
    match route(&request.method, &request.target) {
        Ok(Route::Capabilities) => Request::Capabilities,
        Ok(Route::Status) => Request::Status,
        Ok(Route::CreateJob) => {
            scan_settings(&request.body).map_or(Request::Malformed, Request::CreateJob)
        }
        Ok(Route::NextDocument(id)) => Request::NextDocument(id),
        Ok(Route::DeleteJob(id)) => Request::DeleteJob(id),
        Err(unrouted) => Request::Unrouted(unrouted),
    }
}
