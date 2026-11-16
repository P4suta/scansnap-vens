use core::fmt;

use crate::uuid::Uuid;

const BASE: &str = "/eSCL/";
const JOBS: &str = "ScanJobs";
const NEXT_DOCUMENT: &str = "NextDocument";

/// The path of a job, which a `Location` header and a job's status carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobPath(Uuid);

impl JobPath {
    #[must_use]
    pub const fn new(job: Uuid) -> Self {
        Self(job)
    }

    #[must_use]
    pub const fn job(self) -> Uuid {
        self.0
    }
}

impl fmt::Display for JobPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{BASE}{JOBS}/{}", self.0)
    }
}

/// The eSCL request that a method and a request target name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    Capabilities,
    Status,
    CreateJob,
    NextDocument(Uuid),
    DeleteJob(Uuid),
}

/// The one method that a resource of the bridge allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Delete,
}

impl Method {
    #[must_use]
    pub const fn token(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Delete => "DELETE",
        }
    }
}

/// Why a request names no route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unrouted {
    NotFound,
    /// The path names a resource that allows another method.
    MethodNotAllowed(Method),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Resource {
    Capabilities,
    Status,
    Jobs,
    Job(Uuid),
    NextDocument(Uuid),
}

impl Resource {
    const fn allows(self) -> Method {
        match self {
            Self::Capabilities | Self::Status | Self::NextDocument(_) => Method::Get,
            Self::Jobs => Method::Post,
            Self::Job(_) => Method::Delete,
        }
    }

    const fn route(self) -> Route {
        match self {
            Self::Capabilities => Route::Capabilities,
            Self::Status => Route::Status,
            Self::Jobs => Route::CreateJob,
            Self::Job(id) => Route::DeleteJob(id),
            Self::NextDocument(id) => Route::NextDocument(id),
        }
    }
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
/// Refuses a path outside the five requests, and names the allowed method for a known path with another method.
pub fn route(method: &str, target: &str) -> Result<Route, Unrouted> {
    let path = target.split_once('?').map_or(target, |(path, _)| path);
    let resource = resource(path).ok_or(Unrouted::NotFound)?;
    let allowed = resource.allows();
    if method == allowed.token() {
        Ok(resource.route())
    } else {
        Err(Unrouted::MethodNotAllowed(allowed))
    }
}
