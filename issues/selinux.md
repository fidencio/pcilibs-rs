# sysfs: open existing kernel attributes without O_CREAT

This is **not a P0 for the observed deployment**: the consumer was a short-lived provisioner Job, and the VFIO binding completed successfully. However, the unnecessary SELinux directory-write checks should be addressed for full adoption with confined callers, rather than requiring broader host-policy permissions or accepting avoidable AVCs.

## What happened

While testing kata-device-provisioner with an NVIDIA RTX PRO 6000 Blackwell Server Edition under enforcing SELinux, a fresh VFIO binding succeeded but emitted parent-directory write denials when pcilibs-rs opened these existing kernel attributes:

```text
/sys/bus/pci/devices/0000:9b:00.0/driver_override
/sys/bus/pci/drivers_probe
```

The relevant audit fields were:

```text
path=/sys/bus/pci/devices/0000:9b:00.0/driver_override
syscall=open success=yes exit=3
a1=O_WRONLY|O_CREAT|O_TRUNC|O_CLOEXEC
scontext=system_u:system_r:kata_device_provisioner_t:s0
tcontext=system_u:object_r:sysfs_t:s0
tclass=dir denied={write} permissive=0

path=/sys/bus/pci/drivers_probe
syscall=open success=yes exit=3
a1=O_WRONLY|O_CREAT|O_TRUNC|O_CLOEXEC
scontext=system_u:system_r:kata_device_provisioner_t:s0
tcontext=system_u:object_r:sysfs_t:s0
tclass=dir denied={write} permissive=0
```

These are selected fields from the associated audit events, not complete raw records. Importantly, `success=yes` means the observed opens succeeded despite the AVCs. This report does not claim that those particular denials prevented binding.

The caller's policy intentionally allowed access to existing sysfs control files without granting directory-write access to the broadly used `sysfs_t` type. The opens nevertheless requested `O_CREAT`, causing unnecessary parent-directory write checks on this host.

## Cause and affected paths

The library uses `std::fs::write` for these controls. Rust documents that this convenience function [creates a missing file and replaces an existing file's contents](https://doc.rust-lang.org/std/fs/fn.write.html). Creation is inappropriate for kernel-provided control attributes: if an expected attribute is missing, the operation should fail rather than attempt to create it.

The tested base was `c26e25c520a92463d21281d549142b983014d64d`, pinned by provisioner v0.1.0-alpha.3. The same affected calls were still present in the upstream revision inspected on 2026-11-21:

- [`src/vfio.rs`](https://github.com/kata-containers/pcilibs-rs/blob/55585d4c033dc0c58e82da9603c4055d1f908254/src/vfio.rs): setting/clearing `driver_override`, writing `drivers_probe`, and unbinding a driver.
- [`src/pci_dev.rs`](https://github.com/kata-containers/pcilibs-rs/blob/55585d4c033dc0c58e82da9603c4055d1f908254/src/pci_dev.rs): sysfs reset and runtime-power control/restore.

Fixture setup and generated ordinary files can legitimately need file creation. The proposed change is limited to runtime writes to existing kernel attributes.

## Expected behavior

Open existing kernel attributes for writing without requesting creation. Preserve the existing write/truncation behavior and error context. Missing controls should produce an error, including when using a fake sysfs tree in tests.

This should not require adding `sysfs_t:dir write` to the consumer's SELinux policy. The caller still needs the permissions for its intended file writes, hardware access, and other operations.

## Existing reports checked

Checked on **2026-11-21**. Searches of upstream issues and pull requests for SELinux, `O_CREAT`, `OpenOptions`, `driver_override`, and sysfs work did not identify an open report covering this unnecessary-create behavior.

Related merged work:

- [PR #14: give every sysfs path one root](https://github.com/kata-containers/pcilibs-rs/pull/14) introduced the shared sysfs/VFIO infrastructure and fixtures relevant to these operations.
- [PR #44: sysfs accessors, fixtures, and topology entries](https://github.com/kata-containers/pcilibs-rs/pull/44) adds related accessors and fixture improvements; it does not remove the affected `fs::write` calls in the inspected revision.

Recheck issue and PR status before filing.

## Policy ownership

pcilibs-rs is a library: its operations execute under the calling process's SELinux domain. In this test that was the experimental `kata_device_provisioner_t`, not a pcilibs-specific runtime domain.

No pcilibs-specific policy was found in the inspected library, `container-selinux`, or distribution-policy upstream sources. The host's SELinux policy provides `sysfs_t`; for example, it is defined in upstream [devices.te](https://github.com/fedora-selinux/selinux-policy/blob/9d23da6917bb69fc423ab5771c42aed0a13b9e71/policy/modules/kernel/devices.te). Application/container policy determines the caller's permitted access.

The fix belongs in the library's sysfs-open behavior. It does not propose a new pcilibs SELinux type, relabeling sysfs, or expanding generic container permissions. The complete tested consumer CIL is included in the [provisioner report](https://github.com/fidencio/kata-device-provisioner/blob/confined-selinux-alpha3/issues/selinux-confined-execution.md#tested-cil); it was kept without a sysfs directory-write grant for the successful rerun.

## Environment and validation

The hardware tests ran on AlmaLinux 10.3, kernel `6.12.0-264.16.1.el10_3.x86_64`, Kubernetes v1.37.2, containerd 2.4.2, and an NVIDIA RTX PRO 6000 Blackwell Server Edition (GB202), with SELinux Enforcing:

```text
container-selinux-2.250.0-1.el10.noarch
selinux-policy-42.1.27-2.el10_3.2.noarch
selinux-policy-targeted-42.1.27-2.el10_3.2.noarch
```

An experimental helper opens existing attributes with `OpenOptions` and `write_all`. The VFIO and PCI-device paths above were changed to use it. Fake sysfs fixtures were updated to pre-create the controls that the kernel normally supplies.

Validation against the pinned test base included:

- **419 library unit tests and three doctests** passed, with Clippy warnings denied.
- Regressions verified that a missing attribute is not created, that an existing attribute is overwritten correctly, and that binding refuses missing `driver_override` or `drivers_probe` controls.
- **155 provisioner unit tests** and Clippy passed with the patched library snapshot.
- A fresh VFIO bind on the real GPU completed with no matching provisioner AVCs after the code fix, without adding sysfs directory-write permissions.
- Subsequent real CC `off`/`on`/`devtools` transitions and resets passed through the provisioner, with fresh probes confirming the requested state and no matching provisioner AVCs.
- Generated boot-file labels and baseline contents remained correct. The final GPU state was CC `on`, bound to `vfio-pci`.

Those results validate the exercised consumer paths, not every reset backend or hardware platform. The experimental implementation is preserved in the provisioner branch's [test snapshot and provenance](https://github.com/fidencio/kata-device-provisioner/tree/confined-selinux-alpha3/vendor/pcilibs-rs); it is not an upstream pcilibs release.

## Tested helper

The following helper preserves the existing truncation behavior while leaving creation disabled, as described by the [OpenOptions documentation](https://doc.rust-lang.org/std/fs/struct.OpenOptions.html):

```rust
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::Path;

pub(crate) fn write_attribute(path: impl AsRef<Path>, value: impl AsRef<[u8]>) -> io::Result<()> {
    OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path)?
        .write_all(value.as_ref())
}
```
