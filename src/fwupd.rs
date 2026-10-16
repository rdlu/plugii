//! fwupd's view of the firmware: `fwupdmgr get-devices --json` for the
//! capsule devices the labels name and for failed updates, and `fwupdmgr
//! get-updates --json` for what its cached metadata offers (no network).

use std::time::Duration;

use serde::Deserialize;

use crate::labels::Labels;
use crate::model::{EsrtDevice, FwupdState, PendingUpdate};
use crate::sysfs::command;

/// fwupdmgr talks to the daemon over D-Bus, which can be slow to start.
pub const TIMEOUT: Duration = Duration::from_secs(20);

/// The two commands' JSON, as [`query`] runs them or a test supplies them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub devices: String,
    pub updates: String,
}

/// Runs both commands; `None` when fwupdmgr isn't installed. A command that
/// fails or times out reads as no devices / no updates, as in the script.
pub fn query() -> Option<Output> {
    if !command::on_path("fwupdmgr") {
        return None;
    }
    let run = |what: &str| {
        command::output_any_status("fwupdmgr", &[what, "--json"], TIMEOUT).unwrap_or_default()
    };
    Some(Output {
        devices: run("get-devices"),
        updates: run("get-updates"),
    })
}

#[derive(Debug, Default, Deserialize)]
struct Devices {
    #[serde(rename = "Devices", default)]
    devices: Vec<Device>,
}

#[derive(Debug, Default, Deserialize)]
struct Device {
    #[serde(rename = "Name", default)]
    name: Option<String>,
    #[serde(rename = "Guid", default)]
    guid: Option<Vec<String>>,
    #[serde(rename = "Version", default)]
    version: Option<String>,
    #[serde(rename = "UpdateState", default)]
    update_state: Option<u32>,
    #[serde(rename = "Releases", default)]
    releases: Option<Vec<Release>>,
}

#[derive(Debug, Default, Deserialize)]
struct Release {
    #[serde(rename = "Version", default)]
    version: Option<String>,
}

/// fwupd's `FWUPD_UPDATE_STATE_FAILED` and `…_FAILED_TRANSIENT`.
const FAILED_STATES: [u32; 2] = [3, 5];

/// Unparsable or empty JSON reads as no devices.
fn devices(json: &str) -> Vec<Device> {
    serde_json::from_str::<Devices>(json)
        .map(|d| d.devices)
        .unwrap_or_default()
}

/// The labelled ESRT devices, failed updates and pending updates.
pub fn parse(out: &Output, labels: &Labels) -> FwupdState {
    let devices = devices(&out.devices);
    let labelled = labels
        .esrt
        .iter()
        .filter_map(|(guid, label)| {
            let device = devices.iter().find(|d| {
                d.guid
                    .iter()
                    .flatten()
                    .any(|g| g.eq_ignore_ascii_case(guid))
            })?;
            Some(EsrtDevice {
                guid: guid.clone(),
                label: label.clone(),
                version: device.version.clone().filter(|v| !v.is_empty())?,
            })
        })
        .collect();
    let failed = devices
        .iter()
        .filter(|d| d.update_state.is_some_and(|s| FAILED_STATES.contains(&s)))
        .map(|d| d.name.clone().unwrap_or_else(|| "?".into()))
        .collect();
    let pending = self::devices(&out.updates)
        .into_iter()
        .map(|d| PendingUpdate {
            name: d.name.unwrap_or_else(|| "?".into()),
            version: d.version.unwrap_or_else(|| "?".into()),
            release: d
                .releases
                .and_then(|r| r.into_iter().next())
                .and_then(|r| r.version)
                .unwrap_or_else(|| "?".into()),
        })
        .collect();
    FwupdState {
        labelled,
        failed,
        pending,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn garbage_is_no_devices() {
        let out = Output {
            devices: "Idle…".into(),
            updates: String::new(),
        };
        assert_eq!(parse(&out, &Labels::default()), FwupdState::default());
    }
}
