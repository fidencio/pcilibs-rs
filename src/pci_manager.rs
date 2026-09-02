// Copyright (c) Ant Group
//
// SPDX-License-Identifier: Apache-2.0
//
#![allow(dead_code)]

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::PathBuf;

use crate::pci_ids::{Classes, Vendors};

use crate::{normalize_bdf, Sysfs};

const PCI_CONFIG_SPACE_SZ: u64 = 256;

const UNKNOWN_DEVICE: &str = "UNKNOWN_DEVICE";
const UNKNOWN_CLASS: &str = "UNKNOWN_CLASS";

fn address_to_id(address: &str) -> u64 {
    let cleaned_address = address.replace(":", "").replace(".", "");
    u64::from_str_radix(&cleaned_address, 16).unwrap_or(0)
}

#[derive(Clone, Debug, Default)]
pub struct PCIDevice {
    pub device_path: PathBuf,
    pub address: String,
    pub vendor: u16,
    pub class: u32,
    pub class_name: String,
    pub device: u16,
    pub device_name: String,
    pub driver: String,
    pub iommu_group: i64,
    pub numa_node: i64,
}

#[derive(Clone, Debug, Default)]
pub struct PCIDeviceManager {
    sysfs: Sysfs,
}

impl PCIDeviceManager {
    pub fn new(sysfs: Sysfs) -> Self {
        PCIDeviceManager { sysfs }
    }

    pub fn get_all_devices(&self, vendor: Option<u16>) -> io::Result<Vec<PCIDevice>> {
        let mut pci_devices = Vec::new();
        let device_dirs = fs::read_dir(self.sysfs.devices())?;

        let mut cache: HashMap<String, PCIDevice> = HashMap::new();

        for entry in device_dirs {
            let device_dir = entry?;
            let device_address = device_dir.file_name().to_string_lossy().to_string();
            if let Ok(Some(dev)) =
                self.get_device_by_pci_bus_id(&device_address, vendor, &mut cache)
            {
                pci_devices.push(dev);
            }
        }

        pci_devices.sort_by_key(|dev| address_to_id(&dev.address));

        Ok(pci_devices)
    }

    pub fn get_device_by_pci_bus_id(
        &self,
        address: &str,
        vendor: Option<u16>,
        cache: &mut HashMap<String, PCIDevice>,
    ) -> io::Result<Option<PCIDevice>> {
        let address = normalize_bdf(address).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{address:?} is not a PCI address"),
            )
        })?;

        if let Some(device) = cache.get(&address) {
            return Ok(Some(device.clone()));
        }

        let device_path = self.sysfs.devices().join(&address);

        // read vendor ID
        let vendor_str = fs::read_to_string(device_path.join("vendor"))?;
        let vendor_id = u16::from_str_radix(vendor_str.trim().trim_start_matches("0x"), 16)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        if let Some(vend_id) = vendor {
            if vendor_id != vend_id {
                return Ok(None);
            }
        }

        let class_str = fs::read_to_string(device_path.join("class"))?;
        let class_id = u32::from_str_radix(class_str.trim().trim_start_matches("0x"), 16).unwrap();

        let device_str = fs::read_to_string(device_path.join("device"))?;
        let device_id =
            u16::from_str_radix(device_str.trim().trim_start_matches("0x"), 16).unwrap();

        let driver = match fs::read_link(device_path.join("driver")) {
            Ok(path) => path.file_name().unwrap().to_string_lossy().to_string(),
            Err(_) => String::new(),
        };

        let iommu_group = match fs::read_link(device_path.join("iommu_group")) {
            Ok(path) => path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
                .parse::<i64>()
                .unwrap_or(-1),
            Err(_) => -1,
        };

        let numa_node = fs::read_to_string(device_path.join("numa_node"))
            .map(|numa| numa.trim().parse::<i64>().unwrap_or(-1))
            .unwrap_or(-1);

        let mut device_name = UNKNOWN_DEVICE.to_string();
        for vendor in Vendors::iter() {
            for device in vendor.devices() {
                if vendor.id() == vendor_id && device.id() == device_id {
                    device_name = device.name().to_owned();
                    break;
                }
            }
        }

        let mut class_name = UNKNOWN_CLASS.to_string();
        for class in Classes::iter() {
            if u32::from(class.id()) == class_id {
                class_name = class.name().to_owned();
                break;
            }
        }

        let pci_device = PCIDevice {
            device_path,
            address: address.clone(),
            vendor: vendor_id,
            class: class_id,
            device: device_id,
            driver,
            iommu_group,
            numa_node,
            device_name,
            class_name,
        };

        cache.insert(address, pci_device.clone());

        Ok(Some(pci_device))
    }
}

/// A PCIe function's config space is larger than a conventional PCI one.
pub fn is_pcie_device(bdf: &str, sysfs: &Sysfs) -> bool {
    let Some(device) = sysfs.device(bdf) else {
        return false;
    };

    match fs::metadata(device.join("config")) {
        Ok(metadata) => metadata.len() > PCI_CONFIG_SPACE_SZ,
        // Error reading the file, assume it's not a PCIe device
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    use rstest::rstest;

    // domain number
    const TEST_PCI_DEV_DOMAIN: &str = "0000";

    // Mock data
    fn setup_mock_device_files() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir should not fail");
        // Create mock path and files for PCI devices
        let device_path = Sysfs::new(dir.path()).device("0000:ff:1f.0").unwrap();
        fs::create_dir_all(&device_path).unwrap();
        fs::write(device_path.join("vendor"), "0x8086").unwrap();
        fs::write(device_path.join("device"), "0x1234").unwrap();
        fs::write(device_path.join("class"), "0x060100").unwrap();
        fs::write(device_path.join("numa_node"), "0").unwrap();
        dir
    }

    #[test]
    fn test_get_all_devices() {
        // Setup mock data
        let tmpdir = setup_mock_device_files();

        // Initialize PCI device manager with the mock path
        let manager = PCIDeviceManager::new(Sysfs::new(tmpdir.path()));

        // Get all devices
        let devices_result = manager.get_all_devices(None);

        assert!(devices_result.is_ok());
        let devices = devices_result.unwrap();
        assert_eq!(devices.len(), 1);

        let device = &devices[0];
        assert_eq!(device.vendor, 0x8086);
        assert_eq!(device.device, 0x1234);
        assert_eq!(device.class, 0x060100);
    }

    /// A lookup joins its argument onto the sysfs root, so anything that is
    /// not a PCI address has to be refused rather than followed.
    #[rstest]
    #[case("../../../etc/shadow")]
    #[case("0000:ff:1f.0/../../..")]
    #[case("/etc/shadow")]
    #[case("nonsense")]
    fn a_lookup_refuses_an_address_that_is_not_one(#[case] address: &str) {
        let tmpdir = setup_mock_device_files();
        let manager = PCIDeviceManager::new(Sysfs::new(tmpdir.path()));

        let err = manager
            .get_device_by_pci_bus_id(address, None, &mut HashMap::new())
            .unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    /// Enumeration and direct lookup have to agree on one spelling, or the
    /// cache keys and the addresses handed back drift apart.
    #[test]
    fn a_lookup_canonicalises_the_address_it_reports() {
        let tmpdir = setup_mock_device_files();
        let manager = PCIDeviceManager::new(Sysfs::new(tmpdir.path()));

        let device = manager
            .get_device_by_pci_bus_id("FF:1F.0", None, &mut HashMap::new())
            .unwrap()
            .expect("the mock device should be found");

        assert_eq!(device.address, "0000:ff:1f.0");
    }

    #[test]
    fn test_is_pcie_device() {
        // Create a mock PCI device config file
        let bdf = format!("{TEST_PCI_DEV_DOMAIN}:ff:00.0");
        let tmpdir = tempfile::tempdir().expect("tempdir should not fail");
        let config_path = Sysfs::new(tmpdir.path())
            .device(&bdf)
            .unwrap()
            .join("config");
        let _ = fs::create_dir_all(config_path.parent().unwrap());

        // Write a file with a size larger than PCI_CONFIG_SPACE_SZ
        let mut file = fs::File::create(&config_path).unwrap();
        // Test size greater than PCI_CONFIG_SPACE_SZ
        file.write_all(&vec![0; 512]).unwrap();

        // It should be true
        assert!(is_pcie_device("ff:00.0", &Sysfs::new(tmpdir.path())));
    }
}
