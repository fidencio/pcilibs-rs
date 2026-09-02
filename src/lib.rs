// Copyright (c) Ant Group
//
// SPDX-License-Identifier: Apache-2.0
//

mod iommufd;
mod pci_ids;
mod pci_manager;
mod sysfs;

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::os::unix::prelude::FileTypeExt;

use nix::sys::stat;

#[cfg(feature = "testfs")]
pub use iommufd::testfs;
pub use iommufd::{
    enumerate_iommufd, is_passthrough_capable_class, lookup_iommufd_dev, IommufdDev,
    IOMMUFD_SYSFS_CLASS, IOMMUFD_VFIO_DIR,
};
pub use pci_manager::{is_pcie_device, PCIDevice, PCIDeviceManager};
pub use sysfs::{Sysfs, SYSFS};

/// The PCI domain sysfs always spells out, and callers often omit.
pub const PCI_DEV_DOMAIN: &str = "0000";

/// `65:00.0` and `0000:65:00.0` name the same device; sysfs only answers to
/// the second.
///
/// Rebuilt from the parsed numbers rather than patched up, because the result
/// is joined onto a sysfs root: none of the caller's string may reach a path.
pub fn normalize_bdf(bdf: &str) -> Option<String> {
    let fields: Vec<&str> = bdf.split(':').collect();
    let (domain, bus, slot) = match fields[..] {
        [bus, slot] => (PCI_DEV_DOMAIN, bus, slot),
        [domain, bus, slot] => (domain, bus, slot),
        _ => return None,
    };
    let (device, function) = slot.split_once('.')?;

    let domain = u16::from_str_radix(domain, 16).ok()?;
    let bus = u8::from_str_radix(bus, 16).ok()?;
    let device = u8::from_str_radix(device, 16).ok()?;
    let function = u8::from_str_radix(function, 16).ok()?;

    Some(format!("{domain:04x}:{bus:02x}:{device:02x}.{function:x}"))
}

/// Device driver for vfio-pci guest kernel driver.
pub const DRIVER_VFIO_PCI_GK_TYPE: &str = "vfio-pci-gk";
/// Device driver for vfio-pci.
pub const DRIVER_VFIO_PCI_TYPE: &str = "vfio-pci";
/// Device driver for vfio-ap hotplug.
pub const DRIVER_VFIO_AP_TYPE: &str = "vfio-ap";
/// Device driver for vfio-ap coldplug.
pub const DRIVER_VFIO_AP_COLD_TYPE: &str = "vfio-ap-cold";

pub fn is_vfio_device_type(device_type: &str) -> bool {
    matches!(
        device_type,
        DRIVER_VFIO_PCI_TYPE
            | DRIVER_VFIO_PCI_GK_TYPE
            | DRIVER_VFIO_AP_TYPE
            | DRIVER_VFIO_AP_COLD_TYPE
    )
}

/// One-line summary of every `/sys/class/infiniband*` device the
/// guest kernel currently exposes, plus every char device under
/// `/dev/infiniband/` and the PCI BDF backing each IB device.
///
/// Pure sysfs / devfs reads — no agent-specific dependencies.
/// Used as a diagnostic context string in log calls.
pub fn snapshot_infiniband() -> String {
    let mut ib_parts: Vec<String> = Vec::new();
    if let Ok(entries) = fs::read_dir("/sys/class/infiniband") {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            let pci_bdf = fs::read_link(path.join("device"))
                .ok()
                .and_then(|t| t.file_name().map(|n| n.to_string_lossy().into_owned()))
                .unwrap_or_else(|| "<none>".to_string());
            let node_type = fs::read_to_string(path.join("node_type"))
                .map(|s| s.trim().to_string())
                .unwrap_or_default();
            let fw = fs::read_to_string(path.join("fw_ver"))
                .map(|s| s.trim().to_string())
                .unwrap_or_default();
            ib_parts.push(format!(
                "{name}=[bdf={pci_bdf},node_type={node_type:?},fw={fw}]"
            ));
        }
    }

    let mut verbs_parts: Vec<String> = Vec::new();
    if let Ok(entries) = fs::read_dir("/sys/class/infiniband_verbs") {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with("uverbs") {
                continue;
            }
            let path = entry.path();
            let ibdev = fs::read_to_string(path.join("ibdev"))
                .map(|s| s.trim().to_string())
                .unwrap_or_default();
            let dev = fs::read_to_string(path.join("dev"))
                .map(|s| s.trim().to_string())
                .unwrap_or_default();
            verbs_parts.push(format!("{name}=[ibdev={ibdev},dev={dev}]"));
        }
    }

    let mut chardev_parts: Vec<String> = Vec::new();
    if let Ok(entries) = fs::read_dir("/dev/infiniband") {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let metadata = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            let kind = if metadata.file_type().is_char_device() {
                "char"
            } else if metadata.file_type().is_block_device() {
                "block"
            } else {
                "other"
            };
            let rdev = metadata.rdev();
            let major = stat::major(rdev);
            let minor = stat::minor(rdev);
            chardev_parts.push(format!("{name}=[{kind},{major}:{minor}]"));
        }
    }

    format!(
        "ib_devices=[{}] uverbs=[{}] chardevs=[{}]",
        ib_parts.join(", "),
        verbs_parts.join(", "),
        chardev_parts.join(", "),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[rstest::rstest]
    #[case::already_qualified("0000:65:00.0", "0000:65:00.0")]
    #[case::domain_omitted("65:00.0", "0000:65:00.0")]
    #[case::non_zero_domain("0009:01:00.0", "0009:01:00.0")]
    #[case::upper_case("0009:01:00.0", "0009:01:00.0")]
    #[case::unpadded("9:1:0.0", "0009:01:00.0")]
    fn normalize_bdf_canonicalises_an_address(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(normalize_bdf(input).unwrap(), expected);
    }

    #[rstest::rstest]
    #[case::traversal("../../../etc/shadow")]
    #[case::traversal_shaped_like_an_address("0000:../:00.0")]
    #[case::absolute("/etc/shadow")]
    #[case::separator_in_a_field("0000:65:00.0/../..")]
    #[case::too_few_fields("65")]
    #[case::too_many_fields("0000:0000:65:00.0")]
    #[case::no_function("0000:65:00")]
    #[case::not_hex("zzzz:65:00.0")]
    #[case::empty("")]
    fn normalize_bdf_refuses_what_is_not_an_address(#[case] input: &str) {
        assert!(normalize_bdf(input).is_none(), "{input:?} was accepted");
    }

    #[test]
    fn test_is_vfio_device_type() {
        assert!(is_vfio_device_type(DRIVER_VFIO_PCI_TYPE));
        assert!(is_vfio_device_type(DRIVER_VFIO_PCI_GK_TYPE));
        assert!(is_vfio_device_type(DRIVER_VFIO_AP_TYPE));
        assert!(is_vfio_device_type(DRIVER_VFIO_AP_COLD_TYPE));
        assert!(!is_vfio_device_type("virtio-pci"));
    }
}
