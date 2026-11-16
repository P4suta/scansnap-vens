use core::fmt::{self, Formatter};

use crate::{route::JobPath, uuid::Uuid};

const DECLARATION: &str = r#"<?xml version="1.0" encoding="UTF-8"?>"#;
const NAMESPACES: &str = r#"xmlns:pwg="http://www.pwg.org/schemas/2010/12/sm" xmlns:scan="http://schemas.hp.com/imaging/escl/2011/05/03""#;
const VERSION: &str = "<pwg:Version>2.0</pwg:Version>";
const MAKE_AND_MODEL: &str = "ScanSnap iX1300";
/// The smallest sheet of a U-turn scan in the iX1300 specification, 2 by 2 inches, in 1/300 inch.
const MIN_EDGE: u32 = 600;
/// The scan window at 300 dpi, which every image spans, wider than the 8.5-inch sheet limit.
const MAX_WIDTH: u32 = 2592;
/// The longest sheet of a U-turn scan in the iX1300 specification, 14 inches, in 1/300 inch.
const MAX_HEIGHT: u32 = 4200;
const PROFILE: &str = concat!(
    "<scan:SettingProfiles><scan:SettingProfile>",
    "<scan:ColorModes><scan:ColorMode>RGB24</scan:ColorMode></scan:ColorModes>",
    "<scan:DocumentFormats><pwg:DocumentFormat>image/jpeg</pwg:DocumentFormat></scan:DocumentFormats>",
    "<scan:SupportedResolutions><scan:DiscreteResolutions><scan:DiscreteResolution>",
    "<scan:XResolution>300</scan:XResolution><scan:YResolution>300</scan:YResolution>",
    "</scan:DiscreteResolution></scan:DiscreteResolutions></scan:SupportedResolutions>",
    "</scan:SettingProfile></scan:SettingProfiles>",
);
const INTENTS: &str = concat!(
    "<scan:SupportedIntents>",
    "<scan:SupportedIntent>Document</scan:SupportedIntent>",
    "<scan:SupportedIntent>TextAndGraphic</scan:SupportedIntent>",
    "<scan:SupportedIntent>Photo</scan:SupportedIntent>",
    "</scan:SupportedIntents>",
);

/// The `ScannerCapabilities` document, which offers the feeder alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    uuid: Uuid,
}

impl Capabilities {
    #[must_use]
    pub const fn new(uuid: Uuid) -> Self {
        Self { uuid }
    }
}

fn input_caps(formatter: &mut Formatter<'_>, element: &str) -> fmt::Result {
    write!(
        formatter,
        "<scan:{element}>\
         <scan:MinWidth>{MIN_EDGE}</scan:MinWidth><scan:MaxWidth>{MAX_WIDTH}</scan:MaxWidth>\
         <scan:MinHeight>{MIN_EDGE}</scan:MinHeight><scan:MaxHeight>{MAX_HEIGHT}</scan:MaxHeight>\
         <scan:MaxScanRegions>1</scan:MaxScanRegions>{PROFILE}{INTENTS}</scan:{element}>"
    )
}

impl fmt::Display for Capabilities {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{DECLARATION}\n<scan:ScannerCapabilities {NAMESPACES}>{VERSION}\
             <pwg:MakeAndModel>{MAKE_AND_MODEL}</pwg:MakeAndModel><scan:UUID>{}</scan:UUID><scan:Adf>",
            self.uuid
        )?;
        input_caps(formatter, "AdfSimplexInputCaps")?;
        input_caps(formatter, "AdfDuplexInputCaps")?;
        formatter.write_str(
            "<scan:AdfOptions><scan:AdfOption>Duplex</scan:AdfOption></scan:AdfOptions>\
             </scan:Adf></scan:ScannerCapabilities>\n",
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScannerState {
    Idle,
    Processing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdfState {
    Loaded,
    Empty,
    Jam,
    /// The feeder took more than one sheet at once.
    MultipickDetected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobState {
    Processing,
    Completed,
    Aborted,
    Canceled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobInfo {
    pub id: Uuid,
    pub images_completed: u32,
    pub images_to_transfer: u32,
    pub state: JobState,
}

/// The `ScannerStatus` document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScannerStatus {
    pub state: ScannerState,
    pub adf: AdfState,
    pub job: Option<JobInfo>,
}

const fn scanner_state(state: ScannerState) -> &'static str {
    match state {
        ScannerState::Idle => "Idle",
        ScannerState::Processing => "Processing",
    }
}

const fn adf_state(adf: AdfState) -> &'static str {
    match adf {
        AdfState::Loaded => "ScannerAdfLoaded",
        AdfState::Empty => "ScannerAdfEmpty",
        AdfState::Jam => "ScannerAdfJam",
        AdfState::MultipickDetected => "ScannerAdfMultipickDetected",
    }
}

const fn job_state(state: JobState) -> (&'static str, &'static str) {
    match state {
        JobState::Processing => ("Processing", "JobScanning"),
        JobState::Completed => ("Completed", "JobCompletedSuccessfully"),
        JobState::Aborted => ("Aborted", "AbortedBySystem"),
        JobState::Canceled => ("Canceled", "JobCanceledByUser"),
    }
}

fn job_info(formatter: &mut Formatter<'_>, job: &JobInfo) -> fmt::Result {
    let (state, reason) = job_state(job.state);
    write!(
        formatter,
        "<scan:Jobs><scan:JobInfo>\
         <pwg:JobUri>{}</pwg:JobUri><pwg:JobUuid>{}</pwg:JobUuid>\
         <pwg:ImagesCompleted>{}</pwg:ImagesCompleted><pwg:ImagesToTransfer>{}</pwg:ImagesToTransfer>\
         <pwg:JobState>{state}</pwg:JobState>\
         <pwg:JobStateReasons><pwg:JobStateReason>{reason}</pwg:JobStateReason></pwg:JobStateReasons>\
         </scan:JobInfo></scan:Jobs>",
        JobPath::new(job.id),
        job.id,
        job.images_completed,
        job.images_to_transfer,
    )
}

impl fmt::Display for ScannerStatus {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{DECLARATION}\n<scan:ScannerStatus {NAMESPACES}>{VERSION}\
             <pwg:State>{}</pwg:State><scan:AdfState>{}</scan:AdfState>",
            scanner_state(self.state),
            adf_state(self.adf),
        )?;
        if let Some(job) = &self.job {
            job_info(formatter, job)?;
        }
        formatter.write_str("</scan:ScannerStatus>\n")
    }
}
