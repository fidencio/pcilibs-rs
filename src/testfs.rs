// Copyright (c) 2026 Kata Containers contributors
//
// SPDX-License-Identifier: Apache-2.0

//! Fake kernel trees for tests, in a temp directory.
//!
//! Behind the `testfs` feature so a consumer can build the same fixtures its
//! dependency's own tests use, for dev-dependencies only: this writes fake
//! sysfs trees and belongs nowhere near production code.

use std::fs;
use std::path::Path;

use crate::Sysfs;

/// Add one fake cdev `vfio<n>` with the given sysfs `vendor`, `device`, and
/// `class` contents (as sysfs prints them, e.g. "0x10de", "0x2330",
/// "0x030200").
///
/// `root` is both the `/dev/vfio` and the sysfs root: the cdev lands at
/// `<root>/devices/vfio<n>` and its identity under `<root>/class/vfio-dev/`,
/// so a caller passes `root` and `Sysfs::new(root)` to the same tree.
pub fn add(root: &Path, n: u32, vendor: &str, device: &str, class: &str) {
    let devices = root.join("devices");
    fs::create_dir_all(&devices).unwrap();
    fs::write(devices.join(format!("vfio{n}")), b"").unwrap();
    let dev_dir = Sysfs::new(root)
        .vfio_dev(&format!("vfio{n}"))
        .join("device");
    fs::create_dir_all(&dev_dir).unwrap();
    fs::write(dev_dir.join("vendor"), format!("{vendor}\n")).unwrap();
    fs::write(dev_dir.join("device"), format!("{device}\n")).unwrap();
    fs::write(dev_dir.join("class"), format!("{class}\n")).unwrap();
}
