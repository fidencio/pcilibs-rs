// Copyright (c) 2026 Kata Containers contributors
//
// SPDX-License-Identifier: Apache-2.0

//! Fake kernel trees for tests, in a temp directory.
//!
//! Behind the `testfs` feature so a consumer can build the same fixtures its
//! dependency's own tests use, for dev-dependencies only: this writes fake
//! sysfs trees and belongs nowhere near production code.

use std::fs;
use std::path::{Path, PathBuf};

/// The sysfs root to pass alongside `root` to `enumerate_iommufd`.
pub fn sysfs(root: &Path) -> PathBuf {
    root.join("sysfs")
}

/// Add one fake cdev `vfio<n>` with the given sysfs `vendor`, `device`, and
/// `class` contents (as sysfs prints them, e.g. "0x10de", "0x2330",
/// "0x030200").
pub fn add(root: &Path, n: u32, vendor: &str, device: &str, class: &str) {
    let devices = root.join("devices");
    fs::create_dir_all(&devices).unwrap();
    fs::write(devices.join(format!("vfio{n}")), b"").unwrap();
    let dev_dir = sysfs(root).join(format!("vfio{n}")).join("device");
    fs::create_dir_all(&dev_dir).unwrap();
    fs::write(dev_dir.join("vendor"), format!("{vendor}\n")).unwrap();
    fs::write(dev_dir.join("device"), format!("{device}\n")).unwrap();
    fs::write(dev_dir.join("class"), format!("{class}\n")).unwrap();
}
