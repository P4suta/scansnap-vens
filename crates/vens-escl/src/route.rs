use crate::uuid::Uuid;

const BASE: &str = "/eSCL/";
const JOBS: &str = "ScanJobs";
const NEXT_DOCUMENT: &str = "NextDocument";

/// The eSCL request that a method and a request target name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    Capabilities,
    Status,
    CreateJob,
    NextDocument(Uuid),
    DeleteJob(Uuid),
}

/// Why a request names no route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unrouted {
    NotFound,
    MethodNotAllowed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Resource {
    Capabilities,
    Status,
    Jobs,
    Job(Uuid),
    NextDocument(Uuid),
}

fn resource(path: &str) -> Option<Resource> {
    match path.strip_prefix(BASE)? {
        "ScannerCapabilities" => Some(Resource::Capabilities),
        "ScannerStatus" => Some(Resource::Status),
        JOBS => Some(Resource::Jobs),
        rest => {
            let job = rest.strip_prefix(JOBS)?.strip_prefix('/')?;
            match job.split_once('/') {
                None => Uuid::parse(job).map(Resource::Job),
                Some((id, NEXT_DOCUMENT)) => Uuid::parse(id).map(Resource::NextDocument),
                Some(_) => None,
            }
        }
    }
}

/// Names the eSCL request for a method and an origin-form request target, ignoring any query.
///
/// # Errors
///
/// Refuses a path outside the five requests, and a known path with another method.
pub fn route(method: &str, target: &str) -> Result<Route, Unrouted> {
    let path = target.split_once('?').map_or(target, |(path, _)| path);
    let resource = resource(path).ok_or(Unrouted::NotFound)?;
    match (method, resource) {
        ("GET", Resource::Capabilities) => Ok(Route::Capabilities),
        ("GET", Resource::Status) => Ok(Route::Status),
        ("POST", Resource::Jobs) => Ok(Route::CreateJob),
        ("GET", Resource::NextDocument(id)) => Ok(Route::NextDocument(id)),
        ("DELETE", Resource::Job(id)) => Ok(Route::DeleteJob(id)),
        (
            _,
            Resource::Capabilities
            | Resource::Status
            | Resource::Jobs
            | Resource::Job(_)
            | Resource::NextDocument(_),
        ) => Err(Unrouted::MethodNotAllowed),
    }
}
