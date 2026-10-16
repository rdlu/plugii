//! Lit external DRM connectors, plus the compositor's mode and VRR for them
//! where niri runs.

use std::collections::HashMap;
use std::time::Duration;

use serde_json::Value;

use super::{Root, command, entries};
use crate::model::{Display, DisplayKind, Vrr};

/// Connected connectors under `sys/class/drm`, sorted by name. The built-in
/// panel (eDP) and writeback connectors are left out. `mode` and `vrr` stay
/// `None`; [`apply_niri`] fills them.
pub fn read(root: &Root) -> Vec<Display> {
    let mut displays: Vec<Display> = entries(root.path("sys/class/drm"), |name| {
        connector_name(name).is_some()
    })
    .into_iter()
    .filter(|dir| super::read(dir.join("status")).as_deref() == Some("connected"))
    .filter_map(|dir| {
        let name = dir.file_name()?.to_string_lossy().into_owned();
        let connector = connector_name(&name)?;
        if connector.contains("eDP") || connector.contains("Writeback") {
            return None;
        }
        Some(Display {
            connector: connector.to_owned(),
            kind: kind_of(connector),
            mode: None,
            vrr: None,
        })
    })
    .collect();
    displays.sort_by(|a, b| a.connector.cmp(&b.connector));
    displays
}

/// `card1-DP-3` → `DP-3`; `None` for `card1`, `renderD128`, `version`.
fn connector_name(entry: &str) -> Option<&str> {
    let rest = entry.strip_prefix("card")?;
    let (index, connector) = rest.split_once('-')?;
    (!index.is_empty() && index.bytes().all(|b| b.is_ascii_digit()) && !connector.is_empty())
        .then_some(connector)
}

fn kind_of(connector: &str) -> DisplayKind {
    if connector.starts_with("DP-") {
        DisplayKind::Dp
    } else if connector.starts_with("HDMI-") {
        DisplayKind::Hdmi
    } else {
        DisplayKind::Other
    }
}

/// Output name → (current mode, VRR) from `niri msg --json outputs`. The
/// mode reads like the script's `2560x1440@59.951` (refresh in Hz); VRR is
/// `On` when enabled, `Off` when supported but off, else `Unsupported`.
/// Unparsable input gives an empty map.
pub fn niri_outputs(json: &str) -> HashMap<String, (Option<String>, Option<Vrr>)> {
    let Ok(Value::Object(outputs)) = serde_json::from_str::<Value>(json) else {
        return HashMap::new();
    };
    outputs
        .into_iter()
        .map(|(name, output)| {
            let mode = output
                .get("current_mode")
                .and_then(Value::as_u64)
                .and_then(|i| output.get("modes")?.get(usize::try_from(i).ok()?))
                .and_then(|m| {
                    let width = m.get("width")?.as_u64()?;
                    let height = m.get("height")?.as_u64()?;
                    let refresh_mhz = m.get("refresh_rate")?.as_f64()?;
                    Some(format!("{width}x{height}@{}", refresh_mhz / 1000.0))
                });
            let flag = |key| output.get(key).and_then(Value::as_bool);
            let vrr = match (flag("vrr_enabled"), flag("vrr_supported")) {
                (Some(true), _) => Some(Vrr::On),
                (_, Some(true)) => Some(Vrr::Off),
                (None, None) => None,
                _ => Some(Vrr::Unsupported),
            };
            (name, (mode, vrr))
        })
        .collect()
}

/// Copies niri's mode and VRR onto the displays it names.
pub fn apply_niri(
    displays: &mut [Display],
    outputs: &HashMap<String, (Option<String>, Option<Vrr>)>,
) {
    for display in displays {
        if let Some((mode, vrr)) = outputs.get(&display.connector) {
            display.mode.clone_from(mode);
            display.vrr = *vrr;
        }
    }
}

/// `niri msg --json outputs`, only when `niri` is on `$PATH`; `None` when it
/// isn't, fails (niri not running) or takes over a second.
pub fn query_niri() -> Option<String> {
    if !command::on_path("niri") {
        return None;
    }
    command::output(
        "niri",
        &["msg", "--json", "outputs"],
        Duration::from_secs(1),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connector_names() {
        assert_eq!(connector_name("card1-DP-3"), Some("DP-3"));
        assert_eq!(connector_name("card10-HDMI-A-1"), Some("HDMI-A-1"));
        assert_eq!(connector_name("card1"), None);
        assert_eq!(connector_name("renderD128"), None);
        assert_eq!(connector_name("cardX-DP-1"), None);
    }

    #[test]
    fn kinds() {
        assert_eq!(kind_of("DP-1"), DisplayKind::Dp);
        assert_eq!(kind_of("HDMI-A-1"), DisplayKind::Hdmi);
        assert_eq!(kind_of("DPI-1"), DisplayKind::Other);
        assert_eq!(kind_of("DVI-D-1"), DisplayKind::Other);
    }
}
