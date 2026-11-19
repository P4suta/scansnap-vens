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
    let mut records = String::new();
    for entry in txt_record(uuid) {
        records.push_str("    <txt-record>");
        records.push_str(&entry);
        records.push_str("</txt-record>\n");
    }
    format!(
        "<?xml version=\"1.0\" standalone=\"no\"?>\n\
         <!DOCTYPE service-group SYSTEM \"avahi-service.dtd\">\n\
         <service-group>\n  <name replace-wildcards=\"yes\">{NAME} on %h</name>\n  <service>\n\
         \x20   <type>{SERVICE_TYPE}</type>\n    <port>{port}</port>\n{records}  </service>\n</service-group>\n"
    )
}

/// Registers the eSCL service through the Windows responder.
///
/// # Errors
/// Returns the Windows registration error.
#[cfg(windows)]
pub fn register_with_windows(port: u16, uuid: Uuid) -> std::io::Result<()> {
    windows_dns_sd::register(NAME, SERVICE_TYPE, port, &txt_record(uuid))
}
