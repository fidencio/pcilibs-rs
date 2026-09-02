// Copyright (c) 2026 Kata Containers contributors
//
// SPDX-License-Identifier: Apache-2.0

//! Sysfs paths, off an injectable mount point so tests can point them at a
//! temp tree rather than the running kernel.

use std::path::{Path, PathBuf};

use crate::normalize_bdf;

pub const SYSFS: &str = "/sys";

#[derive(Clone, Debug)]
pub struct Sysfs {
    root: PathBuf,
}

impl Default for Sysfs {
    fn default() -> Self {
        Self::new(Path::new(SYSFS))
    }
}

impl Sysfs {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    pub fn devices(&self) -> PathBuf {
        self.root.join("bus/pci/devices")
    }

    /// `None` unless the address is one, since this is where it becomes a
    /// path.
    pub fn device(&self, address: &str) -> Option<PathBuf> {
        Some(self.devices().join(normalize_bdf(address)?))
    }

    fn class(&self, name: &str) -> PathBuf {
        self.root.join("class").join(name)
    }

    /// `<name>/device` links to the PCI function behind a vfio character
    /// device.
    pub fn vfio_dev(&self, name: &str) -> PathBuf {
        self.class("vfio-dev").join(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use rstest::rstest;

    #[test]
    fn defaults_to_the_running_kernel() {
        assert_eq!(
            Sysfs::default().devices(),
            Path::new("/sys/bus/pci/devices")
        );
    }

    #[test]
    fn derives_every_path_from_the_root_it_was_given() {
        let sysfs = Sysfs::new(Path::new("/tmp/fake"));

        assert_eq!(sysfs.devices(), Path::new("/tmp/fake/bus/pci/devices"));
    }

    #[rstest]
    #[case::canonical("0000:65:00.0")]
    #[case::no_domain("65:00.0")]
    #[case::upper_case("0000:6F:00.0")]
    #[case::unpadded("0:6:0.0")]
    fn spells_an_address_the_way_sysfs_does(#[case] address: &str) {
        let path = Sysfs::default().device(address).expect("a PCI address");

        assert_eq!(path.parent(), Some(Path::new("/sys/bus/pci/devices")));
        assert!(
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().len() == "0000:00:00.0".len()),
            "{path:?}"
        );
    }

    #[rstest]
    #[case::traversal("../../../etc")]
    #[case::absolute("/etc/shadow")]
    #[case::separator("0000:65:00.0/..")]
    #[case::not_hex("zzzz:65:00.0")]
    #[case::empty("")]
    fn refuses_an_address_that_is_not_one(#[case] address: &str) {
        assert!(Sysfs::default().device(address).is_none());
    }
}
