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
fn reads_plugii_labels() {
    let dir = tempfile::tempdir().unwrap();
    let own = dir.path().join("plugii/labels");
    write(&own, XPS);
    assert_eq!(labels::find(dir.path()), Some(own));
    assert_eq!(labels::load(dir.path()).unwrap(), Labels::parse(XPS));
}

/// The usb-ports script's file is not read: plugii keeps its own.
#[test]
fn ignores_usb_ports_labels() {
    let dir = tempfile::tempdir().unwrap();
    write(&dir.path().join("usb-ports/labels"), XPS);
    assert_eq!(labels::find(dir.path()), None);
    assert_eq!(labels::load(dir.path()).unwrap(), Labels::default());
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

#[test]
fn device_labels() {
    let l = Labels::parse(
        "device:05e3:0610  USB switch (desk)\n\
         device:0B95:1790\tEthernet  (desk)\n\
         device:05E3:0610  USB switch (shelf)\n\
         device:5e3:610  too short\n\
         device:05e3-0610  no colon\n\
         device:05e3:061g  not hex\n",
    );
    // Uppercase in the file is the same key; the last line wins.
    assert_eq!(l.device_label(0x05e3, 0x0610), Some("USB switch (shelf)"));
    assert_eq!(l.device_label(0x0b95, 0x1790), Some("Ethernet  (desk)"));
    assert_eq!(l.devices.len(), 2);
    assert_eq!(l.device_label(0x05e3, 0x0626), None);
    // Malformed keys are skipped, not taken for sockets.
    assert!(l.sockets.is_empty());
}

#[test]
fn a_later_empty_value_unlabels_a_device() {
    let l = Labels::parse("device:05e3:0610  USB switch\ndevice:05E3:0610\n");
    assert_eq!(l.device_label(0x05e3, 0x0610), None);
}

#[test]
fn summary_lists_devices() {
    let l = Labels::parse("device:0B95:1790  Ethernet\ndevice:05e3:0610  USB switch (desk)\n");
    assert_eq!(
        l.summary(),
        "\
devices:
  05e3:0610  USB switch (desk)
  0b95:1790  Ethernet
"
    );
}

const FILE: &str = "\
# daisy
0000:06:00.3-1  USB-C (hinge)
# the dock
device:05E3:0610  old switch
0000:06:00.4-1  USB-C (caps lock)
device:05e3:0610  USB switch
typec:port0  USB-C (caps lock)
";

fn device(vendor_id: u16, product_id: u16) -> labels::Key {
    labels::Key::Device {
        vendor_id,
        product_id,
    }
}

fn socket(key: &str) -> labels::Key {
    labels::Key::Socket(key.into())
}

#[test]
fn edit_replaces_the_last_line_in_place() {
    assert_eq!(
        labels::edit(FILE, &socket("0000:06:00.3-1"), "Desk dock"),
        FILE.replace("0000:06:00.3-1  USB-C (hinge)", "0000:06:00.3-1  Desk dock")
    );
    // Of two lines for a device, in either case, the last one (the one that
    // wins) changes; the earlier one stays.
    let out = labels::edit(FILE, &device(0x05e3, 0x0610), "USB switch (desk)");
    assert_eq!(
        out,
        FILE.replace(
            "device:05e3:0610  USB switch",
            "device:05e3:0610  USB switch (desk)"
        )
    );
    assert_eq!(
        Labels::parse(&out).device_label(0x05e3, 0x0610),
        Some("USB switch (desk)")
    );
    let out = labels::edit(FILE, &labels::Key::Typec("port0".into()), "left");
    assert!(out.ends_with("typec:port0  left\n"), "{out}");
}

#[test]
fn edit_matches_an_uppercase_device_key() {
    let text = "# x\ndevice:0B95:1790  AX\n";
    assert_eq!(
        labels::edit(text, &device(0x0b95, 0x1790), "Ethernet"),
        "# x\ndevice:0b95:1790  Ethernet\n"
    );
}

#[test]
fn edit_appends_a_new_key() {
    let out = labels::edit(FILE, &device(0x0b95, 0x1790), "Ethernet (desk)");
    assert_eq!(out, format!("{FILE}device:0b95:1790  Ethernet (desk)\n"));
    // A last line without a newline gets one first; an empty file just the line.
    assert_eq!(
        labels::edit("# x", &socket("0000:00:14.0-1"), "left"),
        "# x\n0000:00:14.0-1  left\n"
    );
    assert_eq!(
        labels::edit("", &socket("0000:00:14.0-1"), "left"),
        "0000:00:14.0-1  left\n"
    );
}

#[test]
fn edit_with_an_empty_value_removes_the_key() {
    let out = labels::edit(FILE, &device(0x05e3, 0x0610), "  ");
    assert_eq!(
        out,
        "\
# daisy
0000:06:00.3-1  USB-C (hinge)
# the dock
0000:06:00.4-1  USB-C (caps lock)
typec:port0  USB-C (caps lock)
"
    );
    assert_eq!(Labels::parse(&out).device_label(0x05e3, 0x0610), None);
    // Nothing to remove: unchanged.
    assert_eq!(labels::edit(FILE, &device(0x0b95, 0x1790), ""), FILE);
}

#[test]
fn edit_keeps_line_endings_and_cleans_the_value() {
    assert_eq!(
        labels::edit(
            "0000:06:00.3-1 a\r\n# c\r\n",
            &socket("0000:06:00.3-1"),
            " two\nlines \t"
        ),
        "0000:06:00.3-1  two lines\r\n# c\r\n"
    );
    // A key that only starts the same is another key.
    assert_eq!(
        labels::edit("0000:06:00.3-10  x\n", &socket("0000:06:00.3-1"), "y"),
        "0000:06:00.3-10  x\n0000:06:00.3-1  y\n"
    );
}

#[test]
fn write_creates_the_directory_and_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = labels::path(dir.path());
    let got = labels::write(
        &path,
        &[(device(0x05e3, 0x0610), "USB switch (desk)".into())],
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "device:05e3:0610  USB switch (desk)\n"
    );
    assert_eq!(got.device_label(0x05e3, 0x0610), Some("USB switch (desk)"));
}

#[test]
fn write_edits_in_place_and_leaves_no_temp_file() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = labels::path(dir.path());
    write(&path, FILE);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let got = labels::write(
        &path,
        &[
            (socket("0000:06:00.4-1"), "left".into()),
            (labels::Key::Typec("port0".into()), "left".into()),
        ],
    )
    .unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert_eq!(text, FILE.replace("USB-C (caps lock)", "left"));
    assert_eq!(got, Labels::parse(&text));
    assert_eq!(got.typec_port_for_socket("0000:06:00.4-1"), Some("port0"));
    let mode = fs::metadata(&path).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
    let names: Vec<_> = fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, ["labels"]);
}

/// A labels file stowed from dotfiles is a symlink: its target changes, the
/// link stays.
#[test]
fn write_goes_through_a_symlink() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("dotfiles/labels");
    write(&target, FILE);
    let path = labels::path(dir.path());
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink("../dotfiles/labels", &path).unwrap();
    labels::write(&path, &[(device(0x0b95, 0x1790), "Ethernet".into())]).unwrap();
    assert!(fs::symlink_metadata(&path).unwrap().is_symlink());
    assert_eq!(
        fs::read_to_string(&target).unwrap(),
        format!("{FILE}device:0b95:1790  Ethernet\n")
    );
}

#[test]
fn display_path_shortens_home() {
    let home = Path::new("/home/u");
    assert_eq!(
        labels::display_path(Path::new("/home/u/.config/plugii/labels"), Some(home)),
        "~/.config/plugii/labels"
    );
    assert_eq!(
        labels::display_path(Path::new("/etc/plugii/labels"), Some(home)),
        "/etc/plugii/labels"
    );
    assert_eq!(
        labels::display_path(Path::new("/home/u/x"), None),
        "/home/u/x"
    );
}
