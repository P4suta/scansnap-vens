//! The bridge serves the five eSCL requests under the `/eSCL` base path that the advertisement names.

use vens_escl::{
    route::{Route, Unrouted, route},
    uuid::Uuid,
};

const JOB: &str = "6f1c2e8a-3b4d-4c5e-9f60-7a8b9c0d1e2f";

fn job() -> Option<Uuid> {
    Uuid::parse(JOB)
}

#[test]
fn each_request_maps_to_its_route() {
    let next = format!("/eSCL/ScanJobs/{JOB}/NextDocument");
    let job_path = format!("/eSCL/ScanJobs/{JOB}");
    assert_eq!(
        [
            route("GET", "/eSCL/ScannerCapabilities"),
            route("GET", "/eSCL/ScannerStatus"),
            route("POST", "/eSCL/ScanJobs"),
            route("GET", &next),
            route("DELETE", &job_path),
        ],
        [
            Ok(Route::Capabilities),
            Ok(Route::Status),
            Ok(Route::CreateJob),
            job().map(Route::NextDocument).ok_or(Unrouted::NotFound),
            job().map(Route::DeleteJob).ok_or(Unrouted::NotFound),
        ]
    );
}

#[test]
fn a_query_string_does_not_change_the_route() {
    assert_eq!(
        route("GET", "/eSCL/ScannerStatus?cache=0"),
        Ok(Route::Status)
    );
}

#[test]
fn a_known_path_with_another_method_is_not_allowed() {
    let job_path = format!("/eSCL/ScanJobs/{JOB}");
    assert_eq!(
        [
            route("POST", "/eSCL/ScannerCapabilities"),
            route("DELETE", "/eSCL/ScannerStatus"),
            route("GET", "/eSCL/ScanJobs"),
            route("GET", &job_path),
            route("get", "/eSCL/ScannerStatus"),
        ],
        [Err(Unrouted::MethodNotAllowed); 5]
    );
}

#[test]
fn any_other_path_is_not_found() {
    let extra = format!("/eSCL/ScanJobs/{JOB}/NextDocument/1");
    let other_base = format!("/escl/ScanJobs/{JOB}/NextDocument");
    assert_eq!(
        [
            route("GET", "/eSCL/ScannerIcon"),
            route("PUT", "/eSCL/ScanBufferInfo"),
            route("GET", "/eSCL/ScanJobs/1/NextDocument"),
            route("GET", "/eSCL/ScanJobs//NextDocument"),
            route("GET", &extra),
            route("GET", &other_base),
            route("GET", "/"),
            route("GET", ""),
        ],
        [Err(Unrouted::NotFound); 8]
    );
}
