//! Native HID operations used only inside a disposable query process.
use super::protocol::{self, Model, Profile, Reading, Wire};
use anyhow::{Result, bail, ensure};
use std::time::{Duration, Instant};
use windows::{
    Win32::{
        Devices::HumanInterfaceDevice::{
            HIDD_ATTRIBUTES, HIDP_CAPS, HidD_FreePreparsedData, HidD_GetAttributes,
            HidD_GetPreparsedData, HidP_GetCaps, PHIDP_PREPARSED_DATA,
        },
        Foundation::{ERROR_IO_PENDING, GENERIC_READ, GENERIC_WRITE, HANDLE, WAIT_OBJECT_0},
        Storage::FileSystem::{
            CreateFileW, FILE_FLAG_OVERLAPPED, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
            ReadFile, WriteFile,
        },
        System::{
            IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED},
            Threading::{CreateEventW, WaitForSingleObject},
        },
    },
    core::{HSTRING, Owned},
};

struct Preparsed(PHIDP_PREPARSED_DATA);
impl Drop for Preparsed {
    fn drop(&mut self) {
        unsafe {
            HidD_FreePreparsedData(self.0);
        }
    }
}
struct Hid {
    handle: Owned<HANDLE>,
    input: usize,
    output: usize,
    until: Instant,
}
impl Hid {
    fn open(path: &str, profile: Profile) -> Result<Self> {
        // Metadata-only handle first; do not send anything before actual USB
        // identity, vendor collection and report bounds have been verified.
        let path = HSTRING::from(path);
        unsafe {
            let metadata = Owned::new(CreateFileW(
                &path,
                0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                None,
            )?);
            let mut attributes = HIDD_ATTRIBUTES {
                Size: size_of::<HIDD_ATTRIBUTES>() as u32,
                ..Default::default()
            };
            ensure!(
                HidD_GetAttributes(*metadata, &mut attributes),
                "HID identity unavailable"
            );
            ensure!(
                attributes.VendorID == profile.vendor && attributes.ProductID == profile.product,
                "HID identity changed"
            );
            let mut data = PHIDP_PREPARSED_DATA::default();
            ensure!(
                HidD_GetPreparsedData(*metadata, &mut data),
                "HID descriptor unavailable"
            );
            let data = Preparsed(data);
            let mut caps = HIDP_CAPS::default();
            HidP_GetCaps(data.0, &mut caps).ok()?;
            ensure!(
                caps.UsagePage == profile.page && caps.Usage == profile.usage,
                "wrong HID collection"
            );
            let (input, output) = (
                usize::from(caps.InputReportByteLength),
                usize::from(caps.OutputReportByteLength),
            );
            ensure!(
                if profile.model == Model::Logitech {
                    input == 20 && output == 20
                } else {
                    (8..=129).contains(&input) && (2..=65).contains(&output)
                },
                "unsupported report sizes"
            );
            let handle = Owned::new(CreateFileW(
                &path,
                GENERIC_READ.0 | GENERIC_WRITE.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                None,
            )?);
            Ok(Self {
                handle,
                input,
                output,
                until: Instant::now() + Duration::from_millis(1250),
            })
        }
    }
    fn transfer(&self, buffer: &mut [u8], write: bool, until: Instant) -> Result<usize> {
        // All buffers and OVERLAPPED stay alive through completion/cancellation.
        // A broken cancellation is ultimately bounded by the process supervisor.
        unsafe {
            let event = Owned::new(CreateEventW(None, true, false, None)?);
            let mut operation = OVERLAPPED {
                hEvent: *event,
                ..Default::default()
            };
            let result = if write {
                WriteFile(*self.handle, Some(buffer), None, Some(&mut operation))
            } else {
                ReadFile(*self.handle, Some(buffer), None, Some(&mut operation))
            };
            if let Err(error) = result {
                if error.code() != ERROR_IO_PENDING.to_hresult() {
                    return Err(error.into());
                }
                let wait = until
                    .saturating_duration_since(Instant::now())
                    .as_millis()
                    .min(150) as u32;
                if WaitForSingleObject(*event, wait) != WAIT_OBJECT_0 {
                    let _ = CancelIoEx(*self.handle, Some(&operation));
                    let mut discarded = 0;
                    let _ = GetOverlappedResult(*self.handle, &operation, &mut discarded, true);
                    bail!("HID operation expired");
                }
            }
            let mut bytes = 0;
            GetOverlappedResult(*self.handle, &operation, &mut bytes, false)?;
            Ok(bytes as usize)
        }
    }
}
impl Wire for Hid {
    fn exchange(&mut self, request: &[u8], matches: impl Fn(&[u8]) -> bool) -> Result<Vec<u8>> {
        ensure!(
            Instant::now() < self.until && request.len() <= self.output,
            "HID query budget exhausted"
        );
        let end = self.until.min(Instant::now() + Duration::from_millis(150));
        let mut output = vec![0; self.output];
        output[..request.len()].copy_from_slice(request);
        ensure!(
            self.transfer(&mut output, true, end)? == self.output,
            "incomplete HID request"
        );
        let mut input = vec![0; self.input];
        for _ in 0..16 {
            ensure!(Instant::now() < end, "HID response expired");
            let count = self.transfer(&mut input, false, end)?;
            if matches(&input[..count]) {
                input.truncate(count);
                return Ok(input);
            }
        }
        bail!("unrelated HID responses exceeded budget")
    }
}

pub fn query(path: &str) -> Result<Vec<Reading>> {
    let profile =
        protocol::profile_path(path).ok_or_else(|| anyhow::anyhow!("unrecognized HID target"))?;
    let mut hid = Hid::open(path, profile)?;
    match profile.model {
        Model::Logitech => {
            let mut found = Vec::new();
            for slot in 1..=6 {
                let nonce = (std::process::id() as u8).wrapping_add(slot);
                if let Ok(Some(reading)) = protocol::logitech(&mut hid, slot, nonce) {
                    found.push(reading);
                }
            }
            Ok(found)
        }
        model => {
            let data = hid.exchange(&[0, 0xb0], |reply| {
                reply.first() == Some(&0xb0)
                    || (reply.first() == Some(&0) && reply.get(1) == Some(&0xb0))
            })?;
            Ok(protocol::nova(model, &data).into_iter().collect())
        }
    }
}
