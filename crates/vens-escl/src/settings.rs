const FEEDER: &str = "Feeder";
const COLOR: &str = "RGB24";
const JPEG: &str = "image/jpeg";
const RESOLUTION: &str = "300";

/// The values of a `ScanSettings` document that the bridge reads, each trimmed of surrounding white space.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Requested<'a> {
    pub input_source: Option<&'a str>,
    pub duplex: Option<&'a str>,
    pub color_mode: Option<&'a str>,
    pub document_format: Option<&'a str>,
    pub document_format_ext: Option<&'a str>,
    pub x_resolution: Option<&'a str>,
    pub y_resolution: Option<&'a str>,
}

/// The sides of each sheet that a job returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sides {
    Front,
    Both,
}

/// The settings of an accepted job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobSettings {
    sides: Sides,
}

impl JobSettings {
    #[must_use]
    pub const fn sides(self) -> Sides {
        self.sides
    }
}

/// The first requested setting that the bridge can't honor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unsupported {
    InputSource,
    Duplex,
    ColorMode,
    DocumentFormat,
    Resolution,
}

fn offered(value: Option<&str>, offer: &str) -> bool {
    value.is_none_or(|value| value == offer)
}

fn sides(duplex: Option<&str>) -> Option<Sides> {
    match duplex {
        None | Some("false" | "0") => Some(Sides::Front),
        Some("true" | "1") => Some(Sides::Both),
        Some(_) => None,
    }
}

/// Accepts the settings that the capabilities offer, and takes the offered value for each absent setting.
///
/// # Errors
///
/// Names the first setting outside the offer.
pub fn accept(requested: &Requested<'_>) -> Result<JobSettings, Unsupported> {
    if !offered(requested.input_source, FEEDER) {
        return Err(Unsupported::InputSource);
    }
    if !offered(requested.color_mode, COLOR) {
        return Err(Unsupported::ColorMode);
    }
    if !(offered(requested.document_format, JPEG) && offered(requested.document_format_ext, JPEG)) {
        return Err(Unsupported::DocumentFormat);
    }
    if !(offered(requested.x_resolution, RESOLUTION) && offered(requested.y_resolution, RESOLUTION))
    {
        return Err(Unsupported::Resolution);
    }
    let sides = sides(requested.duplex).ok_or(Unsupported::Duplex)?;
    Ok(JobSettings { sides })
}
