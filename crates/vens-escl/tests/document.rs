//! The capabilities and status documents, read back with a namespace-aware parser.
//! sane-airscan matches element names by their `pwg:` and `scan:` prefixes, and macOS matches them by namespace, so the documents bind both prefixes and no default namespace.

use roxmltree::Document;
use vens_escl::{
    document::{AdfState, Capabilities, JobInfo, JobState, ScannerState, ScannerStatus},
    uuid::Uuid,
};

const PWG: &str = "http://www.pwg.org/schemas/2010/12/sm";
const SCAN: &str = "http://schemas.hp.com/imaging/escl/2011/05/03";
const ID: &str = "6f1c2e8a-3b4d-4c5e-9f60-7a8b9c0d1e2f";

#[derive(Debug, Clone, Copy)]
enum Space {
    Pwg,
    Scan,
}

use Space::{Pwg, Scan};

const fn uri(space: Space) -> &'static str {
    match space {
        Space::Pwg => PWG,
        Space::Scan => SCAN,
    }
}

fn strings(texts: &[&str]) -> Vec<String> {
    texts.iter().map(|text| (*text).to_owned()).collect()
}

/// Collects the text of every element at the path, which starts at the root element.
/// A document that fails to parse yields nothing, so each test pairs an absent element with a present one.
fn texts(xml: &str, path: &[(Space, &str)]) -> Vec<String> {
    let Ok(document) = Document::parse(xml) else {
        return Vec::new();
    };
    let root = document.root_element();
    let Some(((space, name), steps)) = path.split_first() else {
        return Vec::new();
    };
    if !root.has_tag_name((uri(*space), *name)) {
        return Vec::new();
    }
    let mut nodes = [root].to_vec();
    for (space, name) in steps {
        nodes = nodes
            .iter()
            .flat_map(roxmltree::Node::children)
            .filter(|child| child.has_tag_name((uri(*space), *name)))
            .collect();
    }
    nodes
        .iter()
        .map(|node| node.text().unwrap_or_default().to_owned())
        .collect()
}

fn namespaces(xml: &str) -> Vec<(Option<String>, String)> {
    let Ok(document) = Document::parse(xml) else {
        return Vec::new();
    };
    let mut declared: Vec<_> = document
        .root_element()
        .namespaces()
        .filter(|namespace| namespace.name() != Some("xml"))
        .map(|namespace| {
            (
                namespace.name().map(str::to_owned),
                namespace.uri().to_owned(),
            )
        })
        .collect();
    declared.sort();
    declared
}

fn bound_prefixes() -> Vec<(Option<String>, String)> {
    [
        (Some("pwg".to_owned()), PWG.to_owned()),
        (Some("scan".to_owned()), SCAN.to_owned()),
    ]
    .to_vec()
}

fn capabilities() -> String {
    Uuid::parse(ID).map_or_else(String::new, |id| Capabilities::new(id).to_string())
}

fn path<'a>(base: &[(Space, &'a str)], steps: &[(Space, &'a str)]) -> Vec<(Space, &'a str)> {
    [base, steps].concat()
}

#[test]
fn the_capabilities_bind_both_prefixes_and_no_default_namespace() {
    assert_eq!(namespaces(&capabilities()), bound_prefixes());
}

#[test]
fn the_capabilities_name_their_version_the_scanner_and_its_identifier() {
    let xml = capabilities();
    let root = [(Scan, "ScannerCapabilities")];
    assert_eq!(
        [
            texts(&xml, &path(&root, &[(Pwg, "Version")])),
            texts(&xml, &path(&root, &[(Pwg, "MakeAndModel")])),
            texts(&xml, &path(&root, &[(Scan, "UUID")])),
        ],
        [
            strings(&["2.0"]),
            strings(&["ScanSnap iX1300"]),
            strings(&[ID])
        ]
    );
}

#[test]
fn each_feeder_mode_offers_the_paper_limits_rgb24_jpeg_and_300_dpi() {
    let xml = capabilities();
    for mode in ["AdfSimplexInputCaps", "AdfDuplexInputCaps"] {
        let caps = [(Scan, "ScannerCapabilities"), (Scan, "Adf"), (Scan, mode)];
        let profile = path(
            &caps,
            &[(Scan, "SettingProfiles"), (Scan, "SettingProfile")],
        );
        let resolution = path(
            &profile,
            &[
                (Scan, "SupportedResolutions"),
                (Scan, "DiscreteResolutions"),
                (Scan, "DiscreteResolution"),
            ],
        );
        assert_eq!(
            [
                texts(&xml, &path(&caps, &[(Scan, "MinWidth")])),
                texts(&xml, &path(&caps, &[(Scan, "MaxWidth")])),
                texts(&xml, &path(&caps, &[(Scan, "MinHeight")])),
                texts(&xml, &path(&caps, &[(Scan, "MaxHeight")])),
                texts(&xml, &path(&caps, &[(Scan, "MaxScanRegions")])),
                texts(
                    &xml,
                    &path(&profile, &[(Scan, "ColorModes"), (Scan, "ColorMode")])
                ),
                texts(
                    &xml,
                    &path(
                        &profile,
                        &[(Scan, "DocumentFormats"), (Pwg, "DocumentFormat")]
                    )
                ),
                texts(&xml, &path(&resolution, &[(Scan, "XResolution")])),
                texts(&xml, &path(&resolution, &[(Scan, "YResolution")])),
                texts(
                    &xml,
                    &path(
                        &caps,
                        &[(Scan, "SupportedIntents"), (Scan, "SupportedIntent")]
                    )
                ),
            ],
            [
                strings(&["600"]),
                strings(&["2592"]),
                strings(&["600"]),
                strings(&["4200"]),
                strings(&["1"]),
                strings(&["RGB24"]),
                strings(&["image/jpeg"]),
                strings(&["300"]),
                strings(&["300"]),
                strings(&["Document", "TextAndGraphic", "Photo"]),
            ],
            "{mode}"
        );
    }
}

#[test]
fn the_feeder_offers_duplex_and_the_scanner_offers_no_platen() {
    let xml = capabilities();
    assert_eq!(
        [
            texts(
                &xml,
                &[
                    (Scan, "ScannerCapabilities"),
                    (Scan, "Adf"),
                    (Scan, "AdfOptions"),
                    (Scan, "AdfOption"),
                ],
            ),
            texts(&xml, &[(Scan, "ScannerCapabilities"), (Scan, "Platen")]),
        ],
        [strings(&["Duplex"]), strings(&[])]
    );
}

fn status(state: ScannerState, adf: AdfState, job: Option<JobInfo>) -> String {
    ScannerStatus { state, adf, job }.to_string()
}

fn job(state: JobState) -> Option<JobInfo> {
    Uuid::parse(ID).map(|id| JobInfo {
        id,
        images_completed: 2,
        images_to_transfer: 1,
        state,
    })
}

const STATUS: (Space, &str) = (Scan, "ScannerStatus");
const JOB_INFO: [(Space, &str); 3] = [STATUS, (Scan, "Jobs"), (Scan, "JobInfo")];

#[test]
fn an_idle_status_names_its_version_state_and_feeder_without_jobs() {
    let xml = status(ScannerState::Idle, AdfState::Loaded, None);
    assert_eq!(
        (
            namespaces(&xml),
            [
                texts(&xml, &[STATUS, (Pwg, "Version")]),
                texts(&xml, &[STATUS, (Pwg, "State")]),
                texts(&xml, &[STATUS, (Scan, "AdfState")]),
                texts(&xml, &JOB_INFO),
            ]
        ),
        (
            bound_prefixes(),
            [
                strings(&["2.0"]),
                strings(&["Idle"]),
                strings(&["ScannerAdfLoaded"]),
                strings(&[]),
            ]
        )
    );
}

#[test]
fn a_status_with_a_job_carries_its_path_identifier_counts_and_state() {
    let xml = status(
        ScannerState::Processing,
        AdfState::Loaded,
        job(JobState::Processing),
    );
    let job_uri = format!("/eSCL/ScanJobs/{ID}");
    assert_eq!(
        [
            texts(&xml, &[STATUS, (Pwg, "State")]),
            texts(&xml, &path(&JOB_INFO, &[(Pwg, "JobUri")])),
            texts(&xml, &path(&JOB_INFO, &[(Pwg, "JobUuid")])),
            texts(&xml, &path(&JOB_INFO, &[(Pwg, "ImagesCompleted")])),
            texts(&xml, &path(&JOB_INFO, &[(Pwg, "ImagesToTransfer")])),
            texts(&xml, &path(&JOB_INFO, &[(Pwg, "JobState")])),
            texts(
                &xml,
                &path(
                    &JOB_INFO,
                    &[(Pwg, "JobStateReasons"), (Pwg, "JobStateReason")]
                )
            ),
        ],
        [
            strings(&["Processing"]),
            strings(&[&job_uri]),
            strings(&[ID]),
            strings(&["2"]),
            strings(&["1"]),
            strings(&["Processing"]),
            strings(&["JobScanning"]),
        ]
    );
}

#[test]
fn each_ending_job_state_renders_its_state_and_reason() {
    let rendered = [JobState::Completed, JobState::Aborted, JobState::Canceled].map(|state| {
        let xml = status(ScannerState::Idle, AdfState::Loaded, job(state));
        (
            texts(&xml, &path(&JOB_INFO, &[(Pwg, "JobState")])),
            texts(
                &xml,
                &path(
                    &JOB_INFO,
                    &[(Pwg, "JobStateReasons"), (Pwg, "JobStateReason")],
                ),
            ),
        )
    });
    assert_eq!(
        rendered,
        [
            (
                strings(&["Completed"]),
                strings(&["JobCompletedSuccessfully"])
            ),
            (strings(&["Aborted"]), strings(&["AbortedBySystem"])),
            (strings(&["Canceled"]), strings(&["JobCanceledByUser"])),
        ]
    );
}

#[test]
fn each_feeder_state_renders_its_name() {
    let rendered = [
        AdfState::Loaded,
        AdfState::Empty,
        AdfState::Jam,
        AdfState::MultipickDetected,
    ]
    .map(|adf| {
        texts(
            &status(ScannerState::Idle, adf, None),
            &[STATUS, (Scan, "AdfState")],
        )
    });
    assert_eq!(
        rendered,
        [
            strings(&["ScannerAdfLoaded"]),
            strings(&["ScannerAdfEmpty"]),
            strings(&["ScannerAdfJam"]),
            strings(&["ScannerAdfMultipickDetected"]),
        ]
    );
}
