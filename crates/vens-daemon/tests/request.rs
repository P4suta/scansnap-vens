//! A read HTTP request becomes the bridge's request: its route, and for a job request the settings in its body.
//! The daemon reads `ScanSettings` by namespace, so a client may choose its own prefixes.

use vens_daemon::{http::HttpRequest, request::escl_request};
use vens_escl::{
    bridge::Request,
    route::{Method, Unrouted},
    settings::{Requested, Unsupported, accept},
    uuid::Uuid,
};

const PWG: &str = "http://www.pwg.org/schemas/2010/12/sm";
const SCAN: &str = "http://schemas.hp.com/imaging/escl/2011/05/03";
const JOB: &str = "6f1c2e8a-3b4d-4c5e-9f60-7a8b9c0d1e2f";

fn http(method: &str, target: &str, body: &str) -> HttpRequest {
    HttpRequest {
        method: method.to_owned(),
        target: target.to_owned(),
        user_agent: None,
        content_type: Some("text/xml".to_owned()),
        body: body.as_bytes().to_vec(),
    }
}

fn settings(prefixes: (&str, &str), elements: &str) -> String {
    let (scan, pwg) = prefixes;
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><{scan}:ScanSettings xmlns:{scan}="{SCAN}" xmlns:{pwg}="{PWG}"><{pwg}:Version>2.0</{pwg}:Version>{elements}</{scan}:ScanSettings>"#
    )
}

fn job(body: &str) -> Request {
    escl_request(&http("POST", "/eSCL/ScanJobs", body))
}

fn duplex() -> Request {
    Request::CreateJob(accept(&Requested {
        duplex: Some("true"),
        ..Requested::default()
    }))
}

#[test]
fn each_route_becomes_its_request() {
    let id = Uuid::parse(JOB);
    let next = format!("/eSCL/ScanJobs/{JOB}/NextDocument");
    let job_path = format!("/eSCL/ScanJobs/{JOB}");
    assert_eq!(
        [
            Some(escl_request(&http("GET", "/eSCL/ScannerCapabilities", ""))),
            Some(escl_request(&http("GET", "/eSCL/ScannerStatus", ""))),
            Some(escl_request(&http("GET", &next, ""))),
            Some(escl_request(&http("DELETE", &job_path, ""))),
        ],
        [
            Some(Request::Capabilities),
            Some(Request::Status),
            id.map(Request::NextDocument),
            id.map(Request::DeleteJob),
        ]
    );
}

#[test]
fn an_unknown_path_or_method_keeps_its_refusal() {
    assert_eq!(
        [
            escl_request(&http("GET", "/eSCL/ScannerIcon", "")),
            escl_request(&http("POST", "/eSCL/ScannerStatus", "")),
        ],
        [
            Request::Unrouted(Unrouted::NotFound),
            Request::Unrouted(Unrouted::MethodNotAllowed(Method::Get)),
        ]
    );
}

#[test]
fn the_settings_of_a_duplex_job_request_are_accepted() {
    let body = settings(
        ("scan", "pwg"),
        "<pwg:ScanRegions><pwg:ScanRegion>\
         <pwg:ContentRegionUnits>escl:ThreeHundredthsOfInches</pwg:ContentRegionUnits>\
         <pwg:XOffset>0</pwg:XOffset><pwg:YOffset>0</pwg:YOffset>\
         <pwg:Width>2592</pwg:Width><pwg:Height>4200</pwg:Height>\
         </pwg:ScanRegion></pwg:ScanRegions>\
         <pwg:InputSource>Feeder</pwg:InputSource><scan:ColorMode>RGB24</scan:ColorMode>\
         <pwg:DocumentFormat>image/jpeg</pwg:DocumentFormat>\
         <scan:XResolution>300</scan:XResolution><scan:YResolution>300</scan:YResolution>\
         <scan:Duplex>true</scan:Duplex>",
    );
    assert_eq!(job(&body), duplex());
}

#[test]
fn values_with_surrounding_white_space_and_unknown_elements_still_read() {
    let body = settings(
        ("scan", "pwg"),
        "<scan:Intent>Document</scan:Intent><scan:CompressionFactor>1</scan:CompressionFactor>\
         <pwg:InputSource>\n  Feeder\n</pwg:InputSource><scan:Duplex> true </scan:Duplex>",
    );
    assert_eq!(job(&body), duplex());
}

#[test]
fn other_prefixes_for_the_same_namespaces_read_the_same_settings() {
    let body = settings(
        ("e", "p"),
        "<p:InputSource>Feeder</p:InputSource><e:Duplex>true</e:Duplex>",
    );
    assert_eq!(job(&body), duplex());
}

#[test]
fn an_unsupported_setting_reaches_the_bridge_by_name() {
    let body = settings(
        ("scan", "pwg"),
        "<scan:ColorMode>Grayscale8</scan:ColorMode>",
    );
    assert_eq!(job(&body), Request::CreateJob(Err(Unsupported::ColorMode)));
}

#[test]
fn a_job_request_needs_an_xml_media_type() {
    let body = settings(("scan", "pwg"), "<scan:Duplex>true</scan:Duplex>");
    let typed = |media: Option<&str>| {
        escl_request(&HttpRequest {
            content_type: media.map(str::to_owned),
            ..http("POST", "/eSCL/ScanJobs", &body)
        })
    };
    assert_eq!(
        [
            typed(Some("text/plain")),
            typed(Some("Text/Plain; charset=utf-8")),
            typed(Some("application/x-www-form-urlencoded")),
            typed(Some("multipart/form-data; boundary=x")),
            typed(Some("text/xml")),
            typed(Some("application/xml; charset=utf-8")),
            typed(None),
            typed(Some("application/octet-stream")),
        ],
        [
            Request::Malformed,
            Request::Malformed,
            Request::Malformed,
            Request::Malformed,
            duplex(),
            duplex(),
            Request::Malformed,
            Request::Malformed,
        ]
    );
}

#[test]
fn a_deeply_nested_settings_body_is_malformed() {
    let nested = format!("{}{}", "<a>".repeat(300), "</a>".repeat(300));
    assert_eq!(job(&settings(("scan", "pwg"), &nested)), Request::Malformed);
}

#[test]
fn an_unreadable_settings_body_is_malformed() {
    let foreign_root = format!(r#"<scan:ScanJob xmlns:scan="{SCAN}"/>"#);
    let entity =
        format!(r#"<!DOCTYPE s [<!ENTITY a "Feeder">]><scan:ScanSettings xmlns:scan="{SCAN}"/>"#);
    let not_utf8 = HttpRequest {
        method: "POST".to_owned(),
        target: "/eSCL/ScanJobs".to_owned(),
        user_agent: None,
        content_type: None,
        body: [0xFF, 0xFE, 0x3C].to_vec(),
    };
    assert_eq!(
        [
            job(""),
            job("<scan:ScanSettings"),
            job(&foreign_root),
            job(&entity),
            escl_request(&not_utf8),
        ],
        [Request::Malformed; 5]
    );
}
