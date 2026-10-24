//! Canonical `ratspeak://connection` links: a secret-free way to hand someone
//! the *settings* for a Network -> Connections entry (Internet/TCP or Radio).
//!
//! A link only ever describes what to pre-fill. It never carries an IFAC
//! passphrase, a key, a serial port, a BLE address or any other device-local
//! identifier, and opening one never adds or changes an interface: the
//! frontend shows the normal "Connect" / "Add LoRa Device" sheet pre-filled
//! and the person still has to confirm it themselves.
//!
//! Like `parse_channel_share_target`, this is the single authority for pasted
//! text and native deep links. Native shell integrations must never interpret
//! or persist the raw URL themselves.
//!
//! ```text
//! ratspeak://connection?v=1&kind=tcp&host=rns.example.org&port=4242&name=Example
//! ratspeak://connection?v=1&kind=radio&freq=868000000&bw=125000&sf=8&cr=5&tx=14&name=City
//! ```

use std::net::IpAddr;

use serde::Serialize;
use url::Url;

const CONNECTION_SHARE_SCHEME: &str = "ratspeak";
const CONNECTION_SHARE_HOST: &str = "connection";
const CONNECTION_SHARE_VERSION: &str = "1";
const CONNECTION_SHARE_FORMAT: &str = "ratspeak.connection.v1";
/// Same budget as channel shares so every accepted link can be re-shared as a QR code.
const CONNECTION_SHARE_MAX_BYTES: usize = 230;
/// Matches `maxlength` of the connection name inputs in the dashboard.
const CONNECTION_SHARE_MAX_NAME_CHARS: usize = 32;
const CONNECTION_SHARE_MAX_HOST_BYTES: usize = 253;

pub const CONNECTION_KIND_TCP: &str = "tcp";
pub const CONNECTION_KIND_RADIO: &str = "radio";

/// Typed, secret-free pre-fill values. Exactly one of the `tcp`/`radio` groups
/// is populated, matching `kind`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConnectionShareTarget {
    pub format: &'static str,
    pub payload: String,
    pub kind: &'static str,
    pub name: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub frequency: Option<u64>,
    pub bandwidth: Option<u64>,
    pub spreading_factor: Option<u8>,
    pub coding_rate: Option<u8>,
    pub tx_power: Option<i8>,
}

impl ConnectionShareTarget {
    fn empty(kind: &'static str, name: Option<String>) -> Self {
        Self {
            format: CONNECTION_SHARE_FORMAT,
            payload: String::new(),
            kind,
            name,
            host: None,
            port: None,
            frequency: None,
            bandwidth: None,
            spreading_factor: None,
            coding_rate: None,
            tx_power: None,
        }
    }
}

fn validate_name(name: &str) -> Result<String, String> {
    if name.is_empty()
        || name != name.trim()
        || name.chars().count() > CONNECTION_SHARE_MAX_NAME_CHARS
        || name.chars().any(char::is_control)
    {
        return Err("Connection share name is not valid".into());
    }
    Ok(name.to_string())
}

/// Accept a DNS hostname (lowercase, LDH labels) or an IP literal that is
/// usable as a remote peer. Anything else is rejected rather than "cleaned".
fn validate_host(host: &str) -> Result<String, String> {
    if host.is_empty() || host.len() > CONNECTION_SHARE_MAX_HOST_BYTES {
        return Err("Connection share host is not valid".into());
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        if ip.is_unspecified() || ip.is_multicast() {
            return Err("Connection share host is not a usable peer address".into());
        }
        // Canonical text form only (e.g. `2001:db8::1`, never `2001:0db8::1`).
        return Ok(ip.to_string());
    }
    let valid_label = |label: &str| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    };
    if !host.split('.').all(valid_label) {
        return Err("Connection share host is not valid".into());
    }
    // An all-numeric dotted name that did not parse as IPv4 is a malformed
    // address, not a hostname (`999.1.1.1`, `1.2.3`).
    if host
        .split('.')
        .all(|label| label.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err("Connection share host is not valid".into());
    }
    Ok(host.to_string())
}

fn build_url(target: &ConnectionShareTarget) -> Result<String, String> {
    let mut url = Url::parse(&format!("{CONNECTION_SHARE_SCHEME}://{CONNECTION_SHARE_HOST}"))
        .map_err(|_| "Could not create a connection share".to_string())?;
    {
        let mut query = url.query_pairs_mut();
        query
            .append_pair("v", CONNECTION_SHARE_VERSION)
            .append_pair("kind", target.kind);
        // Fixed field order is part of the canonical form.
        if let Some(host) = &target.host {
            query.append_pair("host", host);
        }
        if let Some(port) = target.port {
            query.append_pair("port", &port.to_string());
        }
        if let Some(value) = target.frequency {
            query.append_pair("freq", &value.to_string());
        }
        if let Some(value) = target.bandwidth {
            query.append_pair("bw", &value.to_string());
        }
        if let Some(value) = target.spreading_factor {
            query.append_pair("sf", &value.to_string());
        }
        if let Some(value) = target.coding_rate {
            query.append_pair("cr", &value.to_string());
        }
        if let Some(value) = target.tx_power {
            query.append_pair("tx", &value.to_string());
        }
        if let Some(name) = &target.name {
            query.append_pair("name", name);
        }
    }
    let payload = url.to_string();
    if payload.len() > CONNECTION_SHARE_MAX_BYTES {
        return Err("Connection share exceeds the supported size".into());
    }
    Ok(payload)
}

/// Build a canonical Internet/TCP connection link.
pub fn build_tcp_connection_share(
    host: &str,
    port: u16,
    name: Option<&str>,
) -> Result<ConnectionShareTarget, String> {
    if port == 0 {
        return Err("Connection share port is not valid".into());
    }
    let name = name.map(validate_name).transpose()?;
    let mut target = ConnectionShareTarget::empty(CONNECTION_KIND_TCP, name);
    target.host = Some(validate_host(host.trim().to_ascii_lowercase().as_str())?);
    target.port = Some(port);
    target.payload = build_url(&target)?;
    Ok(target)
}

/// Build a canonical Radio (LoRa) settings link. Radio links carry only the
/// shared on-air parameters; the device (BLE/USB/TCP RNode) is always chosen
/// locally by the person who opens the link.
pub fn build_radio_connection_share(
    frequency: u64,
    bandwidth: u64,
    spreading_factor: u8,
    coding_rate: u8,
    tx_power: i8,
    name: Option<&str>,
) -> Result<ConnectionShareTarget, String> {
    use ratspeak_core::radio;
    if !(radio::RNODE_FREQUENCY_MIN_HZ..=radio::RNODE_FREQUENCY_MAX_HZ).contains(&frequency) {
        return Err("Connection share radio frequency is out of range".into());
    }
    if !(radio::RNODE_BANDWIDTH_MIN_HZ..=radio::RNODE_BANDWIDTH_MAX_HZ).contains(&bandwidth) {
        return Err("Connection share radio bandwidth is out of range".into());
    }
    if !(radio::RNODE_SPREADING_FACTOR_MIN..=radio::RNODE_SPREADING_FACTOR_MAX)
        .contains(&spreading_factor)
    {
        return Err("Connection share spreading factor is out of range".into());
    }
    if !(radio::RNODE_CODING_RATE_MIN..=radio::RNODE_CODING_RATE_MAX).contains(&coding_rate) {
        return Err("Connection share coding rate is out of range".into());
    }
    if !(radio::RNODE_TX_POWER_MIN_DBM..=radio::RNODE_TX_POWER_MAX_DBM).contains(&tx_power) {
        return Err("Connection share TX power is out of range".into());
    }
    let name = name.map(validate_name).transpose()?;
    let mut target = ConnectionShareTarget::empty(CONNECTION_KIND_RADIO, name);
    target.frequency = Some(frequency);
    target.bandwidth = Some(bandwidth);
    target.spreading_factor = Some(spreading_factor);
    target.coding_rate = Some(coding_rate);
    target.tx_power = Some(tx_power);
    target.payload = build_url(&target)?;
    Ok(target)
}

#[derive(Default)]
struct RawFields {
    version: Option<String>,
    kind: Option<String>,
    host: Option<String>,
    port: Option<String>,
    freq: Option<String>,
    bw: Option<String>,
    sf: Option<String>,
    cr: Option<String>,
    tx: Option<String>,
    name: Option<String>,
}

fn parse_number<T: std::str::FromStr>(raw: Option<&String>, label: &str) -> Result<T, String> {
    raw.ok_or_else(|| format!("Connection share is missing {label}"))?
        .parse::<T>()
        .map_err(|_| format!("Connection share {label} is not valid"))
}

fn unsupported_field(label: &str) -> String {
    format!("{label} contains an unsupported field")
}

/// Parse one canonical, secret-free Ratspeak connection link.
pub fn parse_connection_share_target(payload: &str) -> Result<ConnectionShareTarget, String> {
    if payload != payload.trim() {
        return Err("Connection share is not in canonical form".into());
    }
    if payload.is_empty() || payload.len() > CONNECTION_SHARE_MAX_BYTES {
        return Err("Connection share is empty or too large".into());
    }
    let url =
        Url::parse(payload).map_err(|_| "Not a valid Ratspeak connection share".to_string())?;
    if url.scheme() != CONNECTION_SHARE_SCHEME
        || url.host_str() != Some(CONNECTION_SHARE_HOST)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || !matches!(url.path(), "" | "/")
        || url.fragment().is_some()
    {
        return Err("Not a valid Ratspeak connection share target".into());
    }

    let mut raw = RawFields::default();
    for (key, value) in url.query_pairs() {
        let slot = match key.as_ref() {
            "v" => &mut raw.version,
            "kind" => &mut raw.kind,
            "host" => &mut raw.host,
            "port" => &mut raw.port,
            "freq" => &mut raw.freq,
            "bw" => &mut raw.bw,
            "sf" => &mut raw.sf,
            "cr" => &mut raw.cr,
            "tx" => &mut raw.tx,
            "name" => &mut raw.name,
            // Includes every secret-bearing spelling (`ifac`, `passphrase`,
            // `key`, ...): an unknown field rejects the whole link.
            _ => return Err(unsupported_field("Connection share")),
        };
        if slot.replace(value.into_owned()).is_some() {
            return Err("Connection share repeats a field".into());
        }
    }
    if raw.version.as_deref() != Some(CONNECTION_SHARE_VERSION) {
        return Err("Connection share version is not supported".into());
    }

    let target = match raw.kind.as_deref() {
        Some(CONNECTION_KIND_TCP) => {
            if raw.freq.is_some()
                || raw.bw.is_some()
                || raw.sf.is_some()
                || raw.cr.is_some()
                || raw.tx.is_some()
            {
                return Err("Connection share mixes Internet and Radio fields".into());
            }
            let host = raw
                .host
                .as_deref()
                .ok_or_else(|| "Connection share is missing its host".to_string())?;
            let port: u16 = parse_number(raw.port.as_ref(), "port")?;
            build_tcp_connection_share(host, port, raw.name.as_deref())?
        }
        Some(CONNECTION_KIND_RADIO) => {
            if raw.host.is_some() || raw.port.is_some() {
                return Err("Connection share mixes Internet and Radio fields".into());
            }
            build_radio_connection_share(
                parse_number(raw.freq.as_ref(), "frequency")?,
                parse_number(raw.bw.as_ref(), "bandwidth")?,
                parse_number(raw.sf.as_ref(), "spreading factor")?,
                parse_number(raw.cr.as_ref(), "coding rate")?,
                parse_number(raw.tx.as_ref(), "TX power")?,
                raw.name.as_deref(),
            )?
        }
        _ => return Err("Connection share kind is not supported".into()),
    };
    // Rebuilding from validated values and comparing byte-for-byte rejects
    // non-canonical spellings: uppercase hosts, leading zeros, `+` signs,
    // reordered or percent-encoded-differently fields.
    if target.payload != payload {
        return Err("Connection share is not in canonical form".into());
    }
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TCP: &str = "ratspeak://connection?v=1&kind=tcp&host=rns.example.org&port=4242&name=Example+Hub";
    const RADIO: &str =
        "ratspeak://connection?v=1&kind=radio&freq=868000000&bw=125000&sf=8&cr=5&tx=14&name=City";

    #[test]
    fn tcp_round_trips_through_the_canonical_parser() {
        let target = parse_connection_share_target(TCP).unwrap();
        assert_eq!(target.kind, "tcp");
        assert_eq!(target.host.as_deref(), Some("rns.example.org"));
        assert_eq!(target.port, Some(4242));
        assert_eq!(target.name.as_deref(), Some("Example Hub"));
        assert_eq!(target.frequency, None);
        let rebuilt =
            build_tcp_connection_share("RNS.Example.org ", 4242, Some("Example Hub")).unwrap();
        assert_eq!(rebuilt, target);
        assert_eq!(rebuilt.payload, TCP);
    }

    #[test]
    fn radio_round_trips_and_carries_no_device_identifier() {
        let target = parse_connection_share_target(RADIO).unwrap();
        assert_eq!(target.kind, "radio");
        assert_eq!(
            (
                target.frequency,
                target.bandwidth,
                target.spreading_factor,
                target.coding_rate,
                target.tx_power
            ),
            (Some(868_000_000), Some(125_000), Some(8), Some(5), Some(14))
        );
        assert!(target.host.is_none() && target.port.is_none());
        for forbidden in ["port=", "host=", "ble", "usb", "serial", "ifac", "key", "pass"] {
            assert!(!target.payload.contains(forbidden), "{forbidden}");
        }
    }

    #[test]
    fn ip_literals_use_their_canonical_text_form() {
        let v4 = build_tcp_connection_share("192.0.2.10", 4242, None).unwrap();
        assert_eq!(parse_connection_share_target(&v4.payload).unwrap(), v4);
        let v6 = build_tcp_connection_share("2001:0db8::1", 4242, None).unwrap();
        assert_eq!(v6.host.as_deref(), Some("2001:db8::1"));
        assert_eq!(parse_connection_share_target(&v6.payload).unwrap(), v6);
    }

    #[test]
    fn secrets_and_unknown_fields_reject_the_whole_link() {
        for extra in ["ifac=secret", "passphrase=x", "key=x", "network_name=n", "port2=1"] {
            let link = format!("{TCP}&{extra}");
            let error = parse_connection_share_target(&link).unwrap_err();
            assert!(error.contains("unsupported field"), "{extra}: {error}");
        }
    }

    #[test]
    fn malformed_and_non_canonical_links_are_rejected() {
        let bad = [
            "",
            " ratspeak://connection?v=1&kind=tcp&host=a.example&port=1",
            "ratspeak://connection?v=1&kind=tcp&host=a.example&port=1 ",
            "ratspeak://channel?v=1&kind=tcp&host=a.example&port=1",
            "ratspeak://connection?v=2&kind=tcp&host=a.example&port=1",
            "ratspeak://connection?kind=tcp&host=a.example&port=1",
            "ratspeak://connection?v=1&kind=ble&host=a.example&port=1",
            "ratspeak://connection?v=1&kind=tcp&host=a.example",
            "ratspeak://connection?v=1&kind=tcp&host=a.example&port=0",
            "ratspeak://connection?v=1&kind=tcp&host=a.example&port=65536",
            "ratspeak://connection?v=1&kind=tcp&host=a.example&port=042",
            "ratspeak://connection?v=1&kind=tcp&host=a.example&port=%2B42",
            "ratspeak://connection?v=1&kind=tcp&host=A.Example&port=42",
            "ratspeak://connection?v=1&kind=tcp&host=a.example&port=42&host=b.example",
            "ratspeak://connection?v=1&kind=tcp&port=42&host=a.example",
            "ratspeak://connection?v=1&kind=tcp&host=a.example&port=42&freq=868000000",
            "ratspeak://connection?v=1&kind=tcp&host=a%2Fb.example&port=42",
            "ratspeak://connection?v=1&kind=tcp&host=a.example%3A8080&port=42",
            "ratspeak://connection?v=1&kind=tcp&host=-a.example&port=42",
            "ratspeak://connection?v=1&kind=tcp&host=999.1.1.1&port=42",
            "ratspeak://connection?v=1&kind=tcp&host=0.0.0.0&port=42",
            "ratspeak://connection?v=1&kind=tcp&host=224.0.0.1&port=42",
            "ratspeak://connection?v=1&kind=tcp&host=a.example&port=42&name=%0A",
            "ratspeak://connection?v=1&kind=tcp&host=a.example&port=42&name=",
            "ratspeak://connection?v=1&kind=tcp&host=a.example&port=42#frag",
            "ratspeak://user@connection?v=1&kind=tcp&host=a.example&port=42",
            "ratspeak://connection:80?v=1&kind=tcp&host=a.example&port=42",
            "ratspeak://connection/extra?v=1&kind=tcp&host=a.example&port=42",
            "https://connection/?v=1&kind=tcp&host=a.example&port=42",
        ];
        for link in bad {
            assert!(parse_connection_share_target(link).is_err(), "accepted: {link:?}");
        }
    }

    #[test]
    fn radio_parameters_must_sit_inside_the_runtime_limits() {
        let ok = |f: u64, bw: u64, sf: u8, cr: u8, tx: i8| {
            build_radio_connection_share(f, bw, sf, cr, tx, None).is_ok()
        };
        assert!(ok(868_000_000, 125_000, 8, 5, 14));
        assert!(!ok(136_999_999, 125_000, 8, 5, 14));
        assert!(!ok(3_000_000_001, 125_000, 8, 5, 14));
        assert!(!ok(868_000_000, 7_799, 8, 5, 14));
        assert!(!ok(868_000_000, 1_625_001, 8, 5, 14));
        assert!(!ok(868_000_000, 125_000, 4, 5, 14));
        assert!(!ok(868_000_000, 125_000, 13, 5, 14));
        assert!(!ok(868_000_000, 125_000, 8, 4, 14));
        assert!(!ok(868_000_000, 125_000, 8, 9, 14));
        assert!(!ok(868_000_000, 125_000, 8, 5, -1));
        assert!(!ok(868_000_000, 125_000, 8, 5, 38));
        let missing = "ratspeak://connection?v=1&kind=radio&freq=868000000&bw=125000&sf=8&cr=5";
        assert!(parse_connection_share_target(missing).is_err());
        let mixed = format!("{RADIO}&host=a.example");
        assert!(parse_connection_share_target(&mixed).is_err());
    }

    #[test]
    fn names_are_bounded_and_links_stay_qr_sized() {
        let long = "x".repeat(CONNECTION_SHARE_MAX_NAME_CHARS + 1);
        assert!(build_tcp_connection_share("a.example", 1, Some(&long)).is_err());
        let max = "x".repeat(CONNECTION_SHARE_MAX_NAME_CHARS);
        let target = build_tcp_connection_share("a.example", 1, Some(&max)).unwrap();
        assert!(target.payload.len() <= CONNECTION_SHARE_MAX_BYTES);
        // A maximum-length hostname plus a name cannot overflow the QR budget.
        let host = format!("{}.{}.{}.example", "a".repeat(63), "b".repeat(63), "c".repeat(63));
        assert!(build_tcp_connection_share(&host, 1, Some(&max)).is_err());
    }
}


