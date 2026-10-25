//! Canonical `ratspeak://tcp` and `ratspeak://radio` connection links.
//!
//! These links pre-fill *Network → Connections → Internet/TCP* and
//! *Network → Connections → Radio*. They never create an interface by
//! themselves: the parsed, typed target is handed to the UI, which shows the
//! normal add-connection form and waits for the user to confirm.
//!
//! ```text
//! ratspeak://tcp?v=1&host=rns.example.org&port=4242&name=My%20node
//!               &ifac_name=mynet&ifac_pass=secret            (IFAC optional)
//! ratspeak://radio?v=1&name=Local%20mesh&region=europe&preset=medium_fast
//! ratspeak://radio?v=1&freq=868000000&bw=250000&sf=9&cr=5&tx=14&mode=full
//! ```
//!
//! A radio link carries radio parameters only. The device (BLE address, serial
//! port, USB or TCP endpoint) is local to each phone/computer and is always
//! chosen by the user.
//!
//! This module is the single authority for both pasted/QR links and native
//! deep links. Native shell code must never interpret the raw URL itself.

use std::fmt;
use std::net::Ipv6Addr;

use ratspeak_core::radio;
use serde::Serialize;
use url::Url;

const CONNECTION_LINK_SCHEME: &str = "ratspeak";
const CONNECTION_LINK_HOST_TCP: &str = "tcp";
const CONNECTION_LINK_HOST_RADIO: &str = "radio";
const CONNECTION_LINK_VERSION: &str = "1";
const TCP_LINK_FORMAT: &str = "ratspeak.tcp.v1";
const RADIO_LINK_FORMAT: &str = "ratspeak.radio.v1";

const CONNECTION_LINK_MAX_BYTES: usize = 512;
const NAME_MAX_BYTES: usize = 64;
const IFAC_VALUE_MAX_BYTES: usize = 128;
const HOST_MAX_BYTES: usize = 253;
const DEFAULT_TCP_PORT: u16 = 4242;
const RADIO_MODES: &[&str] = &["full", "gateway", "access_point", "boundary", "roaming"];

/// A validated, typed connection link. Serialises as `{ "kind": "tcp" | "radio", ... }`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConnectionLinkTarget {
    Tcp(TcpLinkTarget),
    Radio(RadioLinkTarget),
}

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct TcpLinkTarget {
    pub format: &'static str,
    pub payload: String,
    pub host: String,
    pub port: u16,
    pub name: Option<String>,
    pub ifac_network_name: Option<String>,
    pub ifac_passphrase: Option<String>,
}

// The IFAC passphrase is a secret: keep it out of logs and assertion output.
impl fmt::Debug for TcpLinkTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TcpLinkTarget")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("name", &self.name)
            .field("ifac_network_name", &self.ifac_network_name)
            .field(
                "ifac_passphrase",
                &self.ifac_passphrase.as_ref().map(|_| "<redacted>"),
            )
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RadioLinkTarget {
    pub format: &'static str,
    pub payload: String,
    pub name: Option<String>,
    pub mode: Option<String>,
    /// Set together with `preset_key` for a named region/preset link.
    pub region_key: Option<String>,
    pub preset_key: Option<String>,
    pub frequency: u64,
    pub bandwidth: u64,
    pub spreading_factor: u8,
    pub coding_rate: u8,
    pub tx_power: i8,
    /// True when the link carried explicit radio parameters instead of a preset.
    pub custom_params: bool,
    pub airtime_limit_short: Option<f64>,
    pub airtime_limit_long: Option<f64>,
}

/// Parse one canonical Ratspeak connection link (TCP or radio).
pub fn parse_connection_link(payload: &str) -> Result<ConnectionLinkTarget, String> {
    let url = parse_envelope(payload)?;
    match url.host_str() {
        Some(CONNECTION_LINK_HOST_TCP) => parse_tcp(payload, &url).map(ConnectionLinkTarget::Tcp),
        Some(CONNECTION_LINK_HOST_RADIO) => {
            parse_radio(payload, &url).map(ConnectionLinkTarget::Radio)
        }
        _ => Err("Not a valid Ratspeak connection link".into()),
    }
}

fn parse_envelope(payload: &str) -> Result<Url, String> {
    if payload != payload.trim() {
        return Err("Connection link is not in canonical form".into());
    }
    if payload.is_empty() || payload.len() > CONNECTION_LINK_MAX_BYTES {
        return Err("Connection link is empty or too large".into());
    }
    let url =
        Url::parse(payload).map_err(|_| "Not a valid Ratspeak connection link".to_string())?;
    if url.scheme() != CONNECTION_LINK_SCHEME
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || !matches!(url.path(), "" | "/")
        || url.fragment().is_some()
    {
        return Err("Not a valid Ratspeak connection link".into());
    }
    Ok(url)
}

/// Collect query pairs, rejecting unknown and repeated fields.
fn collect_fields<'a>(
    url: &Url,
    allowed: &'a [&'static str],
) -> Result<Vec<(&'a str, String)>, String> {
    let mut fields: Vec<(&'a str, String)> = Vec::new();
    for (key, value) in url.query_pairs() {
        let Some(name) = allowed.iter().find(|name| **name == key.as_ref()) else {
            return Err("Connection link contains an unsupported field".into());
        };
        if fields.iter().any(|(seen, _)| seen == name) {
            return Err("Connection link repeats a field".into());
        }
        fields.push((name, value.into_owned()));
    }
    Ok(fields)
}

fn take(fields: &mut Vec<(&str, String)>, key: &str) -> Option<String> {
    let index = fields.iter().position(|(name, _)| *name == key)?;
    Some(fields.swap_remove(index).1)
}

fn require_version(fields: &mut Vec<(&str, String)>) -> Result<(), String> {
    match take(fields, "v").as_deref() {
        Some(CONNECTION_LINK_VERSION) => Ok(()),
        _ => Err("Unsupported connection link version".into()),
    }
}

/// Plain digits only: no sign, no whitespace, no leading zeros.
fn parse_uint(value: &str, field: &str) -> Result<u64, String> {
    let valid = !value.is_empty()
        && value.len() <= 12
        && value.bytes().all(|b| b.is_ascii_digit())
        && (value == "0" || !value.starts_with('0'));
    if !valid {
        return Err(format!(
            "Connection link field `{field}` is not a valid number"
        ));
    }
    value
        .parse::<u64>()
        .map_err(|_| format!("Connection link field `{field}` is not a valid number"))
}

fn ranged<T: TryFrom<u64>>(value: &str, field: &str, min: u64, max: u64) -> Result<T, String> {
    let number = parse_uint(value, field)?;
    if !(min..=max).contains(&number) {
        return Err(format!("Connection link field `{field}` is out of range"));
    }
    T::try_from(number).map_err(|_| format!("Connection link field `{field}` is out of range"))
}

fn parse_percent(value: &str, field: &str) -> Result<f64, String> {
    let well_formed = !value.is_empty()
        && value.len() <= 8
        && value.bytes().all(|b| b.is_ascii_digit() || b == b'.')
        && value.bytes().filter(|b| *b == b'.').count() <= 1
        && !value.starts_with('.')
        && !value.ends_with('.');
    let number = well_formed.then(|| value.parse::<f64>().ok()).flatten();
    match number {
        Some(number) if number > 0.0 && number <= 100.0 => Ok(number),
        _ => Err(format!(
            "Connection link field `{field}` must be a percentage above 0 and up to 100"
        )),
    }
}

fn valid_host(host: &str) -> bool {
    if host.is_empty() || host.len() > HOST_MAX_BYTES {
        return false;
    }
    if host.contains(':') {
        return host.parse::<Ipv6Addr>().is_ok();
    }
    host.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    })
}

/// Interface display names end up as `[[name]]` config sections.
fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= NAME_MAX_BYTES
        && value == value.trim()
        && !value
            .chars()
            .any(|c| c.is_control() || matches!(c, '[' | ']' | '=' | '#'))
}

/// IFAC values are written as `key = value` config lines.
fn valid_ifac_value(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= IFAC_VALUE_MAX_BYTES
        && value == value.trim()
        && !value.chars().any(|c| c.is_control() || c == '#')
}

fn parse_tcp(payload: &str, url: &Url) -> Result<TcpLinkTarget, String> {
    let mut fields = collect_fields(
        url,
        &["v", "host", "port", "name", "ifac_name", "ifac_pass"],
    )?;
    require_version(&mut fields)?;

    let host = take(&mut fields, "host")
        .filter(|host| valid_host(host))
        .ok_or_else(|| "Connection link needs a valid host".to_string())?;
    let port = match take(&mut fields, "port") {
        Some(port) => ranged::<u16>(&port, "port", 1, 65_535)?,
        None => DEFAULT_TCP_PORT,
    };
    let name = take(&mut fields, "name")
        .map(|name| {
            valid_name(&name)
                .then_some(name)
                .ok_or_else(|| "Connection link name is not valid".to_string())
        })
        .transpose()?;
    let ifac_network_name = take(&mut fields, "ifac_name")
        .map(|value| {
            valid_ifac_value(&value)
                .then_some(value)
                .ok_or_else(|| "IFAC network name is not valid".to_string())
        })
        .transpose()?;
    let ifac_passphrase = take(&mut fields, "ifac_pass")
        .map(|value| {
            valid_ifac_value(&value)
                .then_some(value)
                .ok_or_else(|| "IFAC passphrase is not valid".to_string())
        })
        .transpose()?;

    Ok(TcpLinkTarget {
        format: TCP_LINK_FORMAT,
        payload: payload.to_string(),
        host,
        port,
        name,
        ifac_network_name,
        ifac_passphrase,
    })
}

fn parse_radio(payload: &str, url: &Url) -> Result<RadioLinkTarget, String> {
    let mut fields = collect_fields(
        url,
        &[
            "v",
            "name",
            "mode",
            "region",
            "preset",
            "freq",
            "bw",
            "sf",
            "cr",
            "tx",
            "airtime_short",
            "airtime_long",
        ],
    )?;
    require_version(&mut fields)?;

    let name = take(&mut fields, "name")
        .map(|name| {
            valid_name(&name)
                .then_some(name)
                .ok_or_else(|| "Connection link name is not valid".to_string())
        })
        .transpose()?;
    let mode = take(&mut fields, "mode")
        .map(|mode| {
            RADIO_MODES
                .contains(&mode.as_str())
                .then_some(mode)
                .ok_or_else(|| "Connection link has an unknown interface mode".to_string())
        })
        .transpose()?;

    let region = take(&mut fields, "region");
    let preset = take(&mut fields, "preset");
    let freq = take(&mut fields, "freq");
    let bw = take(&mut fields, "bw");
    let sf = take(&mut fields, "sf");
    let cr = take(&mut fields, "cr");
    let tx = take(&mut fields, "tx");

    let named = region.is_some() || preset.is_some();
    let custom = freq.is_some() || bw.is_some() || sf.is_some() || cr.is_some() || tx.is_some();
    if named == custom {
        return Err("Radio link needs either region and preset, or freq, bw, sf, cr and tx".into());
    }

    let (region_key, preset_key, frequency, bandwidth, spreading_factor, coding_rate, tx_power) =
        if named {
            let (Some(region), Some(preset)) = (region, preset) else {
                return Err("Radio link needs both region and preset".into());
            };
            let params = radio::resolve_rnode_params(&region, &preset)
                .ok_or_else(|| "Radio link has an unknown region or preset".to_string())?;
            (
                Some(region),
                Some(preset),
                params.frequency,
                params.bandwidth,
                params.spreading_factor,
                params.coding_rate,
                params.tx_power,
            )
        } else {
            let (Some(freq), Some(bw), Some(sf), Some(cr), Some(tx)) = (freq, bw, sf, cr, tx)
            else {
                return Err("Custom radio link needs freq, bw, sf, cr and tx".into());
            };
            (
                None,
                None,
                ranged::<u64>(
                    &freq,
                    "freq",
                    radio::RNODE_FREQUENCY_MIN_HZ,
                    radio::RNODE_FREQUENCY_MAX_HZ,
                )?,
                ranged::<u64>(
                    &bw,
                    "bw",
                    radio::RNODE_BANDWIDTH_MIN_HZ,
                    radio::RNODE_BANDWIDTH_MAX_HZ,
                )?,
                ranged::<u8>(
                    &sf,
                    "sf",
                    u64::from(radio::RNODE_SPREADING_FACTOR_MIN),
                    u64::from(radio::RNODE_SPREADING_FACTOR_MAX),
                )?,
                ranged::<u8>(
                    &cr,
                    "cr",
                    u64::from(radio::RNODE_CODING_RATE_MIN),
                    u64::from(radio::RNODE_CODING_RATE_MAX),
                )?,
                ranged::<i8>(
                    &tx,
                    "tx",
                    radio::RNODE_TX_POWER_MIN_DBM as u64,
                    radio::RNODE_TX_POWER_MAX_DBM as u64,
                )?,
            )
        };

    let airtime_limit_short = take(&mut fields, "airtime_short")
        .map(|value| parse_percent(&value, "airtime_short"))
        .transpose()?;
    let airtime_limit_long = take(&mut fields, "airtime_long")
        .map(|value| parse_percent(&value, "airtime_long"))
        .transpose()?;

    Ok(RadioLinkTarget {
        format: RADIO_LINK_FORMAT,
        payload: payload.to_string(),
        name,
        mode,
        region_key,
        preset_key,
        frequency,
        bandwidth,
        spreading_factor,
        coding_rate,
        tx_power,
        custom_params: !named,
        airtime_limit_short,
        airtime_limit_long,
    })
}

/// Build a canonical TCP link. Used by share tooling; the result is re-parsed so
/// a link this function returns is always accepted by [`parse_connection_link`].
pub fn build_tcp_link(
    host: &str,
    port: u16,
    name: Option<&str>,
    ifac_network_name: Option<&str>,
    ifac_passphrase: Option<&str>,
) -> Result<TcpLinkTarget, String> {
    let mut url =
        Url::parse("ratspeak://tcp").map_err(|_| "Could not create a link".to_string())?;
    {
        let mut query = url.query_pairs_mut();
        query
            .append_pair("v", CONNECTION_LINK_VERSION)
            .append_pair("host", host)
            .append_pair("port", &port.to_string());
        if let Some(name) = name {
            query.append_pair("name", name);
        }
        if let Some(value) = ifac_network_name {
            query.append_pair("ifac_name", value);
        }
        if let Some(value) = ifac_passphrase {
            query.append_pair("ifac_pass", value);
        }
    }
    match parse_connection_link(url.as_str())? {
        ConnectionLinkTarget::Tcp(target) => Ok(target),
        ConnectionLinkTarget::Radio(_) => Err("Could not create a TCP link".into()),
    }
}

/// Build a canonical radio link from a named region and preset.
pub fn build_radio_preset_link(
    region: &str,
    preset: &str,
    name: Option<&str>,
    mode: Option<&str>,
) -> Result<RadioLinkTarget, String> {
    let mut url =
        Url::parse("ratspeak://radio").map_err(|_| "Could not create a link".to_string())?;
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("v", CONNECTION_LINK_VERSION);
        if let Some(name) = name {
            query.append_pair("name", name);
        }
        if let Some(mode) = mode {
            query.append_pair("mode", mode);
        }
        query
            .append_pair("region", region)
            .append_pair("preset", preset);
    }
    match parse_connection_link(url.as_str())? {
        ConnectionLinkTarget::Radio(target) => Ok(target),
        ConnectionLinkTarget::Tcp(_) => Err("Could not create a radio link".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tcp(link: &str) -> TcpLinkTarget {
        match parse_connection_link(link).expect("valid link") {
            ConnectionLinkTarget::Tcp(target) => target,
            other => panic!("expected tcp, got {other:?}"),
        }
    }

    fn radio(link: &str) -> RadioLinkTarget {
        match parse_connection_link(link).expect("valid link") {
            ConnectionLinkTarget::Radio(target) => target,
            other => panic!("expected radio, got {other:?}"),
        }
    }

    #[test]
    fn tcp_minimal_uses_default_port_and_no_ifac() {
        let target = tcp("ratspeak://tcp?v=1&host=rns.example.org");
        assert_eq!(target.host, "rns.example.org");
        assert_eq!(target.port, 4242);
        assert_eq!(target.name, None);
        assert_eq!(target.ifac_network_name, None);
        assert_eq!(target.ifac_passphrase, None);
    }

    #[test]
    fn tcp_with_ifac_name_and_passphrase() {
        let target = tcp(
            "ratspeak://tcp?v=1&host=10.0.0.5&port=7000&name=My%20node&ifac_name=mynet&ifac_pass=p%40ss%20word",
        );
        assert_eq!(target.host, "10.0.0.5");
        assert_eq!(target.port, 7000);
        assert_eq!(target.name.as_deref(), Some("My node"));
        assert_eq!(target.ifac_network_name.as_deref(), Some("mynet"));
        assert_eq!(target.ifac_passphrase.as_deref(), Some("p@ss word"));
    }

    #[test]
    fn tcp_ifac_may_carry_only_one_of_the_two_fields() {
        let only_name = tcp("ratspeak://tcp?v=1&host=a.example&ifac_name=net");
        assert_eq!(only_name.ifac_passphrase, None);
        let only_pass = tcp("ratspeak://tcp?v=1&host=a.example&ifac_pass=secret");
        assert_eq!(only_pass.ifac_network_name, None);
    }

    #[test]
    fn tcp_accepts_ipv6_literal_and_non_ascii_values_round_trip() {
        let target = tcp("ratspeak://tcp?v=1&host=2001%3Adb8%3A%3A1&port=4242");
        assert_eq!(target.host, "2001:db8::1");
        let built = build_tcp_link(
            "node.example",
            4242,
            Some("Узел"),
            Some("сеть"),
            Some("пароль"),
        )
        .expect("builds");
        assert_eq!(tcp(&built.payload), built);
    }

    #[test]
    fn tcp_debug_output_redacts_the_passphrase() {
        let target = tcp("ratspeak://tcp?v=1&host=a.example&ifac_pass=hunter2");
        assert!(!format!("{target:?}").contains("hunter2"));
    }

    #[test]
    fn tcp_rejects_bad_hosts_ports_and_config_injection() {
        for link in [
            "ratspeak://tcp?v=1",
            "ratspeak://tcp?v=1&host=",
            "ratspeak://tcp?v=1&host=a b.example",
            "ratspeak://tcp?v=1&host=a.example.",
            "ratspeak://tcp?v=1&host=-a.example",
            "ratspeak://tcp?v=1&host=a.example%0Aenabled%3Dno",
            "ratspeak://tcp?v=1&host=a.example%23x",
            "ratspeak://tcp?v=1&host=a.example&port=0",
            "ratspeak://tcp?v=1&host=a.example&port=65536",
            "ratspeak://tcp?v=1&host=a.example&port=+80",
            "ratspeak://tcp?v=1&host=a.example&port=080",
            "ratspeak://tcp?v=1&host=a.example&name=%5B%5Bx%5D%5D",
            "ratspeak://tcp?v=1&host=a.example&name=a%3Db",
            "ratspeak://tcp?v=1&host=a.example&ifac_name=a%0Ab",
            "ratspeak://tcp?v=1&host=a.example&ifac_pass=a%23b",
            "ratspeak://tcp?v=1&host=a.example&ifac_pass=",
        ] {
            assert!(parse_connection_link(link).is_err(), "accepted {link}");
        }
    }

    #[test]
    fn envelope_is_strict() {
        for link in [
            "",
            " ratspeak://tcp?v=1&host=a.example",
            "ratspeak://tcp?v=1&host=a.example ",
            "https://tcp/?v=1&host=a.example",
            "ratspeak://other?v=1&host=a.example",
            "ratspeak://channel?v=1&hub=00112233445566778899aabbccddeeff",
            "ratspeak://user@tcp?v=1&host=a.example",
            "ratspeak://tcp:80?v=1&host=a.example",
            "ratspeak://tcp/extra?v=1&host=a.example",
            "ratspeak://tcp?v=1&host=a.example#frag",
            "ratspeak://tcp?host=a.example",
            "ratspeak://tcp?v=2&host=a.example",
            "ratspeak://tcp?v=1&v=1&host=a.example",
            "ratspeak://tcp?v=1&host=a.example&host=b.example",
            "ratspeak://tcp?v=1&host=a.example&extra=1",
        ] {
            assert!(parse_connection_link(link).is_err(), "accepted {link}");
        }
        let oversized = format!("ratspeak://tcp?v=1&host=a.example&name={}", "x".repeat(600));
        assert!(parse_connection_link(&oversized).is_err());
    }

    #[test]
    fn tcp_trailing_slash_path_is_accepted() {
        assert_eq!(tcp("ratspeak://tcp/?v=1&host=a.example").port, 4242);
    }

    #[test]
    fn radio_named_region_and_preset_resolve_parameters() {
        let target = radio(
            "ratspeak://radio?v=1&name=Local%20mesh&region=europe&preset=medium_fast&mode=gateway",
        );
        let expected = radio::resolve_rnode_params("europe", "medium_fast").unwrap();
        assert!(!target.custom_params);
        assert_eq!(target.region_key.as_deref(), Some("europe"));
        assert_eq!(target.preset_key.as_deref(), Some("medium_fast"));
        assert_eq!(target.frequency, expected.frequency);
        assert_eq!(target.bandwidth, expected.bandwidth);
        assert_eq!(target.spreading_factor, expected.spreading_factor);
        assert_eq!(target.mode.as_deref(), Some("gateway"));
        assert_eq!(target.name.as_deref(), Some("Local mesh"));
    }

    #[test]
    fn radio_custom_parameters_and_airtime_limits() {
        let target = radio(
            "ratspeak://radio?v=1&freq=868000000&bw=250000&sf=9&cr=5&tx=14&airtime_short=1.5&airtime_long=10",
        );
        assert!(target.custom_params);
        assert_eq!(target.region_key, None);
        assert_eq!(
            (
                target.frequency,
                target.bandwidth,
                target.spreading_factor,
                target.coding_rate,
                target.tx_power
            ),
            (868_000_000, 250_000, 9, 5, 14)
        );
        assert_eq!(target.airtime_limit_short, Some(1.5));
        assert_eq!(target.airtime_limit_long, Some(10.0));
    }

    #[test]
    fn radio_rejects_mixed_partial_unknown_and_out_of_range() {
        for link in [
            "ratspeak://radio?v=1",
            "ratspeak://radio?v=1&region=europe",
            "ratspeak://radio?v=1&preset=medium_fast",
            "ratspeak://radio?v=1&region=nowhere&preset=medium_fast",
            "ratspeak://radio?v=1&region=europe&preset=nope",
            "ratspeak://radio?v=1&region=europe&preset=medium_fast&freq=868000000",
            "ratspeak://radio?v=1&freq=868000000&bw=250000&sf=9&cr=5",
            "ratspeak://radio?v=1&freq=1&bw=250000&sf=9&cr=5&tx=14",
            "ratspeak://radio?v=1&freq=868000000&bw=250000&sf=4&cr=5&tx=14",
            "ratspeak://radio?v=1&freq=868000000&bw=250000&sf=13&cr=5&tx=14",
            "ratspeak://radio?v=1&freq=868000000&bw=250000&sf=9&cr=9&tx=14",
            "ratspeak://radio?v=1&freq=868000000&bw=250000&sf=9&cr=5&tx=38",
            "ratspeak://radio?v=1&freq=868000000&bw=250000&sf=9&cr=5&tx=-1",
            "ratspeak://radio?v=1&region=europe&preset=medium_fast&mode=turbo",
            "ratspeak://radio?v=1&region=europe&preset=medium_fast&airtime_long=0",
            "ratspeak://radio?v=1&region=europe&preset=medium_fast&airtime_long=101",
            "ratspeak://radio?v=1&region=europe&preset=medium_fast&airtime_long=NaN",
            "ratspeak://radio?v=1&region=europe&preset=medium_fast&port=ble%3A%2F%2Faa",
            "ratspeak://radio?v=1&region=europe&preset=medium_fast&ifac_pass=x",
        ] {
            assert!(parse_connection_link(link).is_err(), "accepted {link}");
        }
    }

    #[test]
    fn radio_builder_round_trips() {
        let built =
            build_radio_preset_link("europe", "medium_fast", Some("Local mesh"), Some("full"))
                .expect("builds");
        assert_eq!(radio(&built.payload), built);
    }
}
