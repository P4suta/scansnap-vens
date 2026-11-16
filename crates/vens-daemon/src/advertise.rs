use std::fmt::Write as _;

use vens_escl::uuid::Uuid;

/// The DNS-SD service type of eSCL over plain HTTP.
pub const SERVICE_TYPE: &str = "_uscan._tcp";
const NAME: &str = "ScanSnap iX1300";

/// Lists the TXT record entries, with the record version first as RFC 6763, section 6.7, recommends.
#[must_use]
pub fn txt_record(uuid: Uuid) -> [String; 9] {
    [
        "txtvers=1".to_owned(),
        "vers=2.0".to_owned(),
        "rs=eSCL".to_owned(),
        format!("ty={NAME}"),
        format!("uuid={uuid}"),
        "pdl=image/jpeg".to_owned(),
        "cs=color".to_owned(),
        "is=adf".to_owned(),
        "duplex=T".to_owned(),
    ]
}

/// Renders an Avahi static service file that advertises the bridge on `port`.
#[must_use]
pub fn avahi_service(port: u16, uuid: Uuid) -> String {
    let records = txt_record(uuid)
        .iter()
        .fold(String::new(), |mut records, entry| {
            let _ = writeln!(records, "    <txt-record>{entry}</txt-record>");
            records
        });
    format!(
        "<?xml version=\"1.0\" standalone=\"no\"?>\n\
         <!DOCTYPE service-group SYSTEM \"avahi-service.dtd\">\n\
         <service-group>\n  <name replace-wildcards=\"yes\">{NAME} on %h</name>\n  <service>\n\
         \x20   <type>{SERVICE_TYPE}</type>\n    <port>{port}</port>\n{records}  </service>\n</service-group>\n"
    )
}
