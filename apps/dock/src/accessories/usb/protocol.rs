//! Read-only protocol facts, independently implemented. References and device
//! limits live in ACCESSORIES.md. No settings, pairing or firmware commands.

use anyhow::{Result, ensure};
use muelle_shell::accessories::Kind;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Profile {
    pub vendor: u16,
    pub product: u16,
    pub page: u16,
    pub usage: u16,
    pub model: Model,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Model {
    Logitech,
    NovaDiscrete,
    NovaPercent,
}

pub const PROFILES: [Profile; 6] = [
    Profile {
        vendor: 0x046d,
        product: 0xc52b,
        page: 0xff00,
        usage: 2,
        model: Model::Logitech,
    },
    Profile {
        vendor: 0x046d,
        product: 0xc532,
        page: 0xff00,
        usage: 2,
        model: Model::Logitech,
    },
    Profile {
        vendor: 0x046d,
        product: 0xc548,
        page: 0xff00,
        usage: 2,
        model: Model::Logitech,
    },
    Profile {
        vendor: 0x1038,
        product: 0x2202,
        page: 0xffc0,
        usage: 1,
        model: Model::NovaDiscrete,
    },
    Profile {
        vendor: 0x1038,
        product: 0x22a1,
        page: 0xffc0,
        usage: 1,
        model: Model::NovaPercent,
    },
    Profile {
        vendor: 0x1038,
        product: 0x227e,
        page: 0xffc0,
        usage: 1,
        model: Model::NovaPercent,
    },
];

pub fn profile_path(path: &str) -> Option<Profile> {
    if path.len() > 1024 || path.contains('\0') {
        return None;
    }
    let path = path.to_ascii_lowercase();
    if !path.starts_with(r"\\?\hid#vid_") {
        return None;
    }
    PROFILES.into_iter().find(|profile| {
        let prefix = format!(
            r"\\?\hid#vid_{:04x}&pid_{:04x}&",
            profile.vendor, profile.product
        );
        path.starts_with(&prefix)
            && (profile.model == Model::Logitech
                || path
                    .split('#')
                    .nth(1)
                    .is_some_and(|hardware| hardware.split('&').any(|part| part == "mi_03")))
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reading {
    pub slot: u8,
    pub kind: Kind,
    pub percent: Option<u8>,
}

pub trait Wire {
    fn exchange(&mut self, request: &[u8], matches: impl Fn(&[u8]) -> bool) -> Result<Vec<u8>>;
}

const SOFTWARE: u8 = 0x0c;
pub fn hidpp_request(slot: u8, feature: u8, function: u8, parameters: &[u8]) -> [u8; 20] {
    let mut packet = [0; 20];
    packet[..4].copy_from_slice(&[0x11, slot, feature, function << 4 | SOFTWARE]);
    let count = parameters.len().min(16);
    packet[4..4 + count].copy_from_slice(&parameters[..count]);
    packet
}
fn hidpp_call(
    wire: &mut impl Wire,
    slot: u8,
    feature: u8,
    function: u8,
    parameters: &[u8],
) -> Result<Vec<u8>> {
    let request = hidpp_request(slot, feature, function, parameters);
    let reply = wire.exchange(&request, |reply| hidpp_matches(&request, reply))?;
    ensure!(reply[2] == feature, "HID++ query rejected");
    Ok(reply[4..].to_vec())
}
pub fn hidpp_matches(request: &[u8; 20], reply: &[u8]) -> bool {
    reply.len() == 20
        && reply[0] == 0x11
        && reply[1] == request[1]
        && ((reply[2] == request[2] && reply[3] == request[3])
            || (reply[2] == 0xff && reply[3] == request[2] && reply[4] == request[3]))
}
fn feature(wire: &mut impl Wire, slot: u8, id: u16) -> Result<u8> {
    let answer = hidpp_call(wire, slot, 0, 0, &id.to_be_bytes())?;
    Ok(if answer[1] & 0x80 == 0 { answer[0] } else { 0 })
}

pub fn logitech(wire: &mut impl Wire, slot: u8, nonce: u8) -> Result<Option<Reading>> {
    let ping = hidpp_call(wire, slot, 0, 1, &[0, 0, nonce])?;
    ensure!(
        matches!(ping[0], 2 | 4) && ping[2] == nonce,
        "invalid HID++ ping"
    );
    let name_feature = feature(wire, slot, 0x0005)?;
    if name_feature == 0 {
        return Ok(None);
    }
    let device_type = hidpp_call(wire, slot, name_feature, 2, &[])?;
    let kind = match device_type[0] {
        3..=5 => Kind::Mouse,
        8 => Kind::Headset,
        _ => return Ok(None),
    };
    let battery = feature(wire, slot, 0x1004)?;
    let percent = if battery == 0 {
        None
    } else {
        let capabilities = hidpp_call(wire, slot, battery, 0, &[])?;
        let status = hidpp_call(wire, slot, battery, 1, &[])?;
        unified_percent(&capabilities, &status)
    };
    Ok(Some(Reading {
        slot,
        kind,
        percent,
    }))
}

pub fn unified_percent(capabilities: &[u8], status: &[u8]) -> Option<u8> {
    // Bit 1 advertises an actual state of charge. Level flags are not percent.
    (capabilities.get(1)? & 2 != 0 && status.len() >= 4 && status[2] <= 3)
        .then(|| status[0])
        .filter(|level| *level <= 100)
}

pub fn nova(model: Model, raw: &[u8]) -> Option<Reading> {
    let body = if raw.first() == Some(&0) {
        &raw[1..]
    } else {
        raw
    };
    if body.len() < 7 || body[0] != 0xb0 || !matches!(body[3], 1..=3) {
        return None;
    }
    let percent = match model {
        Model::NovaPercent if body[2] <= 100 => Some(body[2]),
        Model::NovaDiscrete if body[2] <= 4 => None,
        _ => return None,
    };
    Some(Reading {
        slot: 0,
        kind: Kind::Headset,
        percent,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Script(std::collections::VecDeque<([u8; 20], Vec<u8>)>);
    impl Wire for Script {
        fn exchange(&mut self, request: &[u8], matches: impl Fn(&[u8]) -> bool) -> Result<Vec<u8>> {
            let (expected, payload) = self
                .0
                .pop_front()
                .ok_or_else(|| anyhow::anyhow!("unexpected device command"))?;
            assert_eq!(request, expected);
            let mut reply = expected;
            reply[4..].fill(0);
            reply[4..4 + payload.len()].copy_from_slice(&payload);
            assert!(matches(&reply));
            Ok(reply.to_vec())
        }
    }
    #[test]
    fn receiver_discovery_sends_only_queries_and_preserves_an_exact_zero() -> Result<()> {
        let mut wire = Script(std::collections::VecDeque::from([
            (hidpp_request(1, 0, 1, &[0, 0, 42]), vec![4, 0, 42]),
            (hidpp_request(1, 0, 0, &[0, 5]), vec![3, 0, 0]),
            (hidpp_request(1, 3, 2, &[]), vec![3]),
            (hidpp_request(1, 0, 0, &[0x10, 4]), vec![5, 0, 0]),
            (hidpp_request(1, 5, 0, &[]), vec![15, 2]),
            (hidpp_request(1, 5, 1, &[]), vec![0, 1, 0, 0]),
        ]));
        let result = logitech(&mut wire, 1, 42)?;
        assert_eq!(
            result,
            Some(Reading {
                slot: 1,
                kind: Kind::Mouse,
                percent: Some(0)
            })
        );
        assert!(wire.0.is_empty());
        Ok(())
    }
    #[test]
    fn unknown_models_and_wrong_interfaces_are_never_probed() {
        assert!(profile_path(r"\\?\hid#vid_1038&pid_2202&mi_03#instance").is_some());
        for path in [
            r"\\?\hid#vid_1038&pid_2202&mi_00#instance",
            r"\\?\hid#vid_046d&pid_ffff&mi_02#instance",
            r"\\server\hid#vid_046d&pid_c52b&mi_02",
            r"C:\data",
        ] {
            assert!(profile_path(path).is_none());
        }
    }
    #[test]
    fn replies_must_belong_to_the_slot_feature_and_software() {
        let request = hidpp_request(1, 3, 1, &[]);
        let mut response = request;
        assert!(hidpp_matches(&request, &response));
        for index in [0, 1, 2, 3] {
            response = request;
            response[index] ^= 1;
            assert!(!hidpp_matches(&request, &response));
        }
        assert!(!hidpp_matches(&request, &request[..6]));
    }
    #[test]
    fn approximate_levels_errors_and_zero_remain_distinct() {
        assert_eq!(unified_percent(&[15, 2], &[0, 1, 0, 0]), Some(0));
        assert_eq!(unified_percent(&[15, 0], &[0, 8, 0, 0]), None);
        assert_eq!(unified_percent(&[15, 2], &[101, 0, 0, 0]), None);
        assert_eq!(unified_percent(&[15, 2], &[50, 0, 4, 0]), None);
        let raw = [0, 0xb0, 0, 0, 3, 0, 0, 0];
        assert_eq!(
            nova(Model::NovaPercent, &raw).and_then(|r| r.percent),
            Some(0)
        );
        assert_eq!(
            nova(Model::NovaDiscrete, &raw).and_then(|r| r.percent),
            None
        );
        let mut offline = raw;
        offline[4] = 0;
        assert!(nova(Model::NovaPercent, &offline).is_none());
        assert!(nova(Model::NovaPercent, &[0xb0, 3]).is_none());
    }
}
