//! The bridge accepts the settings that its capabilities offer: the feeder, RGB24, JPEG images, and 300 dpi.
//! Each refusal names the first setting that the bridge can't honor, and every other setting holds.

use vens_escl::settings::{JobSettings, Requested, Sides, Unsupported, accept};

const OFFERED: Requested<'static> = Requested {
    input_source: Some("Feeder"),
    duplex: Some("true"),
    color_mode: Some("RGB24"),
    document_format: Some("image/jpeg"),
    document_format_ext: None,
    x_resolution: Some("300"),
    y_resolution: Some("300"),
};

fn sides(requested: &Requested<'_>) -> Result<Sides, Unsupported> {
    accept(requested).map(JobSettings::sides)
}

#[test]
fn the_offered_settings_scan_both_sides() {
    assert_eq!(sides(&OFFERED), Ok(Sides::Both));
}

#[test]
fn each_form_of_duplex_selects_the_sides() {
    let forms = ["true", "1", "false", "0"].map(|duplex| {
        sides(&Requested {
            duplex: Some(duplex),
            ..OFFERED
        })
    });
    assert_eq!(
        forms,
        [
            Ok(Sides::Both),
            Ok(Sides::Both),
            Ok(Sides::Front),
            Ok(Sides::Front)
        ]
    );
}

#[test]
fn absent_settings_take_the_offered_defaults_and_scan_one_side() {
    assert_eq!(sides(&Requested::default()), Ok(Sides::Front));
}

#[test]
fn a_matching_format_extension_is_accepted() {
    assert_eq!(
        sides(&Requested {
            document_format: None,
            document_format_ext: Some("image/jpeg"),
            ..OFFERED
        }),
        Ok(Sides::Both)
    );
}

#[test]
fn each_setting_outside_the_offer_is_refused_by_name() {
    let refusals = [
        Requested {
            input_source: Some("Platen"),
            ..OFFERED
        },
        Requested {
            duplex: Some("yes"),
            ..OFFERED
        },
        Requested {
            color_mode: Some("Grayscale8"),
            ..OFFERED
        },
        Requested {
            color_mode: Some("BlackAndWhite1"),
            ..OFFERED
        },
        Requested {
            document_format: Some("application/pdf"),
            ..OFFERED
        },
        Requested {
            document_format_ext: Some("image/png"),
            ..OFFERED
        },
        Requested {
            x_resolution: Some("600"),
            ..OFFERED
        },
        Requested {
            y_resolution: Some("300.0"),
            ..OFFERED
        },
    ]
    .map(|requested| sides(&requested));
    assert_eq!(
        refusals,
        [
            Err(Unsupported::InputSource),
            Err(Unsupported::Duplex),
            Err(Unsupported::ColorMode),
            Err(Unsupported::ColorMode),
            Err(Unsupported::DocumentFormat),
            Err(Unsupported::DocumentFormat),
            Err(Unsupported::Resolution),
            Err(Unsupported::Resolution),
        ]
    );
}
