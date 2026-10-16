use std::path::PathBuf;

use plugii::model::{Display, DisplayKind, Vrr};
use plugii::sysfs::{Root, drm};

fn fixture(name: &str) -> Root {
    Root::new(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/sysfs")
            .join(name),
    )
}

fn display(connector: &str, kind: DisplayKind) -> Display {
    Display {
        connector: connector.into(),
        kind,
        mode: None,
        vrr: None,
    }
}

#[test]
fn lit_dp_alt_mode_on_intel_laptop() {
    assert_eq!(
        drm::read(&fixture("intel-ac-usbc")),
        vec![display("DP-3", DisplayKind::Dp)]
    );
}

#[test]
fn edp_and_writeback_are_left_out() {
    assert_eq!(
        drm::read(&fixture("amd-battery")),
        vec![display("HDMI-A-1", DisplayKind::Hdmi)]
    );
}

#[test]
fn desktop_connectors_sorted_by_name() {
    assert_eq!(
        drm::read(&fixture("desktop")),
        vec![
            display("DP-1", DisplayKind::Dp),
            display("DVI-D-1", DisplayKind::Other),
            display("HDMI-A-1", DisplayKind::Hdmi),
        ]
    );
}

#[test]
fn no_drm_class_means_no_displays() {
    assert_eq!(drm::read(&fixture("energy-unreadable")), vec![]);
}

/// `niri msg --json outputs` from the dev machine (serials removed, mode
/// lists cut short): an external RTK monitor on DP-3 with VRR on, the panel.
const NIRI_OUTPUTS: &str = r#"{
  "DP-3": {
    "name": "DP-3",
    "make": "Invalid Vendor Codename - RTK",
    "model": "HG540T40",
    "serial": null,
    "physical_size": null,
    "modes": [
      {"width": 2560, "height": 1600, "refresh_rate": 60002, "is_preferred": true},
      {"width": 2560, "height": 1440, "refresh_rate": 59951, "is_preferred": false},
      {"width": 1920, "height": 1080, "refresh_rate": 60000, "is_preferred": false}
    ],
    "current_mode": 1,
    "is_custom_mode": false,
    "vrr_supported": true,
    "vrr_enabled": true,
    "logical": {"x": 107, "y": 0, "width": 1706, "height": 1066, "scale": 1.5, "transform": "Normal"}
  },
  "eDP-1": {
    "name": "eDP-1",
    "make": "LG Display",
    "model": "0x06CE",
    "serial": null,
    "physical_size": [290, 180],
    "modes": [{"width": 1920, "height": 1200, "refresh_rate": 59950, "is_preferred": true}],
    "current_mode": 0,
    "is_custom_mode": false,
    "vrr_supported": true,
    "vrr_enabled": false,
    "logical": {"x": 0, "y": 1067, "width": 1920, "height": 1200, "scale": 1.0, "transform": "Normal"}
  },
  "HDMI-A-1": {
    "name": "HDMI-A-1",
    "modes": [{"width": 1920, "height": 1080, "refresh_rate": 60000, "is_preferred": true}],
    "current_mode": 0,
    "vrr_supported": false,
    "vrr_enabled": false,
    "logical": null
  },
  "DP-5": {
    "name": "DP-5",
    "modes": [{"width": 1920, "height": 1080, "refresh_rate": 60000, "is_preferred": true}],
    "current_mode": null,
    "vrr_supported": true,
    "vrr_enabled": false,
    "logical": null
  }
}"#;

#[test]
fn niri_outputs_mode_and_vrr() {
    let outputs = drm::niri_outputs(NIRI_OUTPUTS);
    assert_eq!(
        outputs.get("DP-3"),
        Some(&(Some("2560x1440@59.951".to_owned()), Some(Vrr::On)))
    );
    assert_eq!(
        outputs.get("eDP-1"),
        Some(&(Some("1920x1200@59.95".to_owned()), Some(Vrr::Off)))
    );
    assert_eq!(
        outputs.get("HDMI-A-1"),
        Some(&(Some("1920x1080@60".to_owned()), Some(Vrr::Unsupported)))
    );
    // Disabled output: no current mode.
    assert_eq!(outputs.get("DP-5"), Some(&(None, Some(Vrr::Off))));
    assert_eq!(outputs.len(), 4);
}

#[test]
fn niri_outputs_tolerates_garbage() {
    assert!(drm::niri_outputs("").is_empty());
    assert!(drm::niri_outputs("not json").is_empty());
    assert!(drm::niri_outputs("[1, 2]").is_empty());
}

#[test]
fn apply_niri_fills_mode_and_vrr() {
    let mut displays = drm::read(&fixture("intel-ac-usbc"));
    drm::apply_niri(&mut displays, &drm::niri_outputs(NIRI_OUTPUTS));
    assert_eq!(
        displays,
        vec![Display {
            connector: "DP-3".into(),
            kind: DisplayKind::Dp,
            mode: Some("2560x1440@59.951".into()),
            vrr: Some(Vrr::On),
        }]
    );
}
