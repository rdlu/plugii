use std::fs;
use std::path::PathBuf;

use plugii::model::{KernelLog, LinkResets};
use plugii::sysfs::kernel_log::{self, usb_location};

fn logs(name: &str) -> (String, String) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/kernel_log")
        .join(name);
    (
        fs::read_to_string(dir.join("warnings.log")).unwrap(),
        fs::read_to_string(dir.join("all.log")).unwrap(),
    )
}

/// Sockets as the usb reader would label them: root ports 1-1 and 2-1 are
/// the two halves of the left USB-C socket, 1-2 a USB-A one.
fn socket_of(msg: &str) -> Option<String> {
    match usb_location(msg)? {
        (1, 1) | (2, 1) => Some("USB-C (left)".into()),
        (1, 2) => Some("USB-A (right)".into()),
        _ => None,
    }
}

#[test]
fn healthy_xps_boot_is_clean() {
    let (warnings, all) = logs("xps");
    assert_eq!(
        kernel_log::parse(&warnings, &all, socket_of),
        KernelLog {
            errors: vec![],
            benign_count: 0,
            link_resets: vec![],
        }
    );
}

#[test]
fn every_error_kind_benign_lines_and_resets() {
    let (warnings, all) = logs("faults");
    let log = kernel_log::parse(&warnings, &all, socket_of);
    insta::assert_json_snapshot!(log);
    assert_eq!(log.errors.len(), 11);
    assert_eq!(log.benign_count, 3);
}

#[test]
fn resets_grouped_by_socket_then_port() {
    let (warnings, all) = logs("faults");
    assert_eq!(
        kernel_log::parse(&warnings, &all, socket_of).link_resets,
        vec![
            LinkResets {
                place: "USB-A (right)".into(),
                count: 1
            },
            LinkResets {
                place: "USB-C (left)".into(),
                count: 5
            },
            LinkResets {
                place: "port 4-2".into(),
                count: 1
            },
            LinkResets {
                place: "unknown port".into(),
                count: 1
            },
        ]
    );
}

#[test]
fn without_socket_labels_resets_fall_back_to_ports() {
    let (warnings, all) = logs("faults");
    let places: Vec<_> = kernel_log::parse(&warnings, &all, |_| None)
        .link_resets
        .into_iter()
        .map(|r| (r.place, r.count))
        .collect();
    assert_eq!(
        places,
        vec![
            ("port 1-1".to_owned(), 2),
            ("port 1-2".to_owned(), 1),
            ("port 2-1".to_owned(), 3),
            ("port 4-2".to_owned(), 1),
            ("unknown port".to_owned(), 1),
        ]
    );
}

#[test]
fn empty_input() {
    assert_eq!(
        kernel_log::parse("", "", |_| None),
        KernelLog {
            errors: vec![],
            benign_count: 0,
            link_resets: vec![],
        }
    );
}
