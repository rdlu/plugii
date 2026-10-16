//! `--firmware`: DMI, the vendor catalog and fwupd assembled into a
//! [`Firmware`]. The network ([`Fetch`]), the cache directory ([`Cache`]) and
//! fwupdmgr's output ([`FwupdInput`]) come in from the caller, so fixture
//! tests run offline.

use crate::fwupd;
use crate::labels::Labels;
use crate::model::{Firmware, Fwupd};
use crate::sysfs::Root;
use crate::vendor::{self, Cache, Fetch};

/// Where fwupd's state comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FwupdInput {
    /// Run fwupdmgr.
    Run,
    /// This output; `None` stands for fwupdmgr not being installed.
    Given(Option<fwupd::Output>),
}

pub fn collect(
    root: &Root,
    labels: &Labels,
    fetch: &dyn Fetch,
    cache: &Cache,
    fwupd_input: FwupdInput,
) -> Firmware {
    let dmi = vendor::read_dmi(root);
    let catalog = vendor::catalog(&dmi, fetch, cache);
    let output = match fwupd_input {
        FwupdInput::Run => fwupd::query(),
        FwupdInput::Given(output) => output,
    };
    let fwupd = match output {
        Some(out) => Fwupd::Checked(fwupd::parse(&out, labels)),
        None => Fwupd::NotInstalled,
    };
    Firmware {
        dmi,
        catalog,
        fwupd,
    }
}
