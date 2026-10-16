use std::fs;
use std::path::Path;

use plugii::labels::{self, Labels};

const XPS: &str = include_str!("fixtures/labels/xps");

#[test]
fn xps_sockets() {
    let l = Labels::parse(XPS);
    assert_eq!(l.sockets.len(), 2);
    assert_eq!(l.socket_label("0000:00:14.0-1"), Some("USB-C (left)"));
    assert_eq!(l.socket_label("0000:00:14.0-3"), Some("USB-C (right)"));
    assert_eq!(l.socket_label("0000:00:14.0-2"), None);
}

#[test]
fn xps_peers() {
    let l = Labels::parse(XPS);
    assert_eq!(l.peers.len(), 2);
    assert_eq!(l.peer_of("0000:00:14.0-1"), Some("0000:00:0d.0-1"));
    assert_eq!(l.peer_of("0000:00:14.0-3"), Some("0000:00:0d.0-3"));
    assert_eq!(l.peer_of("0000:00:0d.0-1"), None);
}

#[test]
fn xps_typec() {
    let l = Labels::parse(XPS);
    assert_eq!(l.typec.len(), 2);
    assert_eq!(l.typec_label("port0"), Some("USB-C (left)"));
    assert_eq!(l.typec_label("port1"), Some("USB-C (right)"));
    assert_eq!(l.typec_port_for_socket("0000:00:14.0-1"), Some("port0"));
    assert_eq!(l.typec_port_for_socket("0000:00:14.0-3"), Some("port1"));
    assert_eq!(l.typec_port_for_socket("0000:00:14.0-2"), None);
}

#[test]
fn xps_has_no_esrt() {
    let l = Labels::parse(XPS);
    assert!(l.esrt.is_empty());
}

#[test]
fn comments_and_blank_lines_are_skipped() {
    let l = Labels::parse("\n   \n# a comment\n  #indented comment\n#x 0000:00:14.0-1 nope\n\t\n");
    assert_eq!(l, Labels::default());
}

#[test]
fn hash_inside_a_value_is_kept() {
    let l = Labels::parse("0000:06:00.4-1  USB-C #2\n");
    assert_eq!(l.socket_label("0000:06:00.4-1"), Some("USB-C #2"));
}

#[test]
fn value_keeps_inner_spaces_and_trims_the_ends() {
    let l = Labels::parse("  0000:06:00.4-1 \t USB-C  (caps   lock)  \t\n");
    assert_eq!(
        l.socket_label("0000:06:00.4-1"),
        Some("USB-C  (caps   lock)")
    );
}

#[test]
fn last_duplicate_wins() {
    let l = Labels::parse(
        "0000:06:00.4-1 first\n0000:06:00.4-1 second\ntypec:port0 a\ntypec:port0 b\n",
    );
    assert_eq!(l.socket_label("0000:06:00.4-1"), Some("second"));
    assert_eq!(l.typec_label("port0"), Some("b"));
}

#[test]
fn a_later_empty_value_unlabels_the_key() {
    // bash stores the empty string, and `${label[$key]:-$key}` treats it as unset.
    let l = Labels::parse("0000:06:00.4-1 first\n0000:06:00.4-1\n");
    assert_eq!(l.socket_label("0000:06:00.4-1"), None);
}

#[test]
fn peer_takes_the_first_token_of_the_value() {
    let l = Labels::parse("peer:0000:00:14.0-1  0000:00:0d.0-1  same port number\n");
    assert_eq!(l.peer_of("0000:00:14.0-1"), Some("0000:00:0d.0-1"));
    assert!(l.sockets.is_empty());
}

#[test]
fn esrt_labels() {
    let l = Labels::parse("esrt:a77b326c-1234-5678-9abc-def012345678  USB-C PD firmware\n");
    assert_eq!(
        l.esrt_label("a77b326c-1234-5678-9abc-def012345678"),
        Some("USB-C PD firmware")
    );
}

#[test]
fn typec_port_for_unlabelled_socket_is_none() {
    let l = Labels::parse("typec:port0 USB-C (left)\n");
    assert_eq!(l.typec_port_for_socket("0000:00:14.0-1"), None);
}

#[test]
fn crlf_line_endings() {
    let l = Labels::parse("0000:06:00.4-1 USB-C\r\ntypec:port0 USB-C\r\n");
    assert_eq!(l.socket_label("0000:06:00.4-1"), Some("USB-C"));
    assert_eq!(l.typec_port_for_socket("0000:06:00.4-1"), Some("port0"));
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

#[test]
fn missing_file_is_empty_labels() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(labels::find(dir.path()), None);
    assert_eq!(labels::load(dir.path()).unwrap(), Labels::default());
}

#[test]
fn falls_back_to_usb_ports_labels() {
    let dir = tempfile::tempdir().unwrap();
    let legacy = dir.path().join("usb-ports/labels");
    write(&legacy, XPS);
    assert_eq!(labels::find(dir.path()), Some(legacy));
    assert_eq!(labels::load(dir.path()).unwrap(), Labels::parse(XPS));
}

#[test]
fn plugii_labels_win_over_usb_ports() {
    let dir = tempfile::tempdir().unwrap();
    let own = dir.path().join("plugii/labels");
    write(&own, "0000:06:00.4-1 mine\n");
    write(&dir.path().join("usb-ports/labels"), XPS);
    assert_eq!(labels::find(dir.path()), Some(own));
    let l = labels::load(dir.path()).unwrap();
    assert_eq!(l.socket_label("0000:06:00.4-1"), Some("mine"));
    assert_eq!(l.socket_label("0000:00:14.0-1"), None);
}

#[test]
fn config_dir_honours_xdg_then_home() {
    let at = |xdg: Option<&str>, home: Option<&str>| {
        labels::config_dir_from(xdg.map(Into::into), home.map(Into::into))
    };
    assert_eq!(at(Some("/x/cfg"), Some("/home/u")), Some("/x/cfg".into()));
    assert_eq!(at(None, Some("/home/u")), Some("/home/u/.config".into()));
    // Empty or relative XDG_CONFIG_HOME is ignored, as the XDG spec says.
    assert_eq!(
        at(Some(""), Some("/home/u")),
        Some("/home/u/.config".into())
    );
    assert_eq!(
        at(Some("rel"), Some("/home/u")),
        Some("/home/u/.config".into())
    );
    assert_eq!(at(None, None), None);
}

#[test]
fn summary_of_xps() {
    let summary = Labels::parse(XPS).summary();
    assert_eq!(
        summary,
        "\
sockets:
  0000:00:14.0-1  USB-C (left)  (USB 3: 0000:00:0d.0-1, USB-C: port0)
  0000:00:14.0-3  USB-C (right)  (USB 3: 0000:00:0d.0-3, USB-C: port1)
"
    );
}

#[test]
fn summary_lists_what_no_socket_shows() {
    let l = Labels::parse(
        "0000:06:00.4-1 USB-C (caps lock)\n\
         typec:port0 USB-C (caps lock)\n\
         typec:port1 USB-C (rear)\n\
         peer:0000:00:14.0-2 0000:00:0d.0-2\n\
         esrt:a77b326c USB-C PD firmware\n",
    );
    assert_eq!(
        l.summary(),
        "\
sockets:
  0000:06:00.4-1  USB-C (caps lock)  (USB-C: port0)
unlabeled peers:
  0000:00:14.0-2 -> 0000:00:0d.0-2
USB-C ports:
  port1  USB-C (rear)
firmware (ESRT):
  a77b326c  USB-C PD firmware
"
    );
    assert_eq!(Labels::default().summary(), "no labels\n");
}
