"""Boot Twine's native ABI 11 communication checks on the pinned RAM-only platform."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import struct
import subprocess
import time

PAGE = 4096
TARGET = "riscv64gc-unknown-none-elf"
PASS = b"PASS twine-share abi=11 clone=1 reply=1 teams=2 wrap=1 drain=1 revoke=1 exit=1"
TAG = "26.11.14-preview-Gamma"


def initrd(elf: bytes) -> bytes:
    assert elf[:6] == b"\x7fELF\x02\x01"
    entry, table = struct.unpack_from("<QQ", elf, 24)
    width, count = struct.unpack_from("<HH", elf, 54)
    output, regions = bytearray(PAGE), []
    for i in range(count):
        kind, flags, offset, address, _, length, size, _ = struct.unpack_from("<IIQQQQQQ", elf, table + i * width)
        if kind != 1 or size == 0:
            continue
        assert address % PAGE == 0 and length <= size
        pages, data = (size + PAGE - 1) // PAGE, (length + PAGE - 1) // PAGE
        payload = len(output) if data else 0
        if data:
            output.extend(elf[offset:offset + length])
            output.extend(bytes(payload + data * PAGE - len(output)))
        regions.append((address, pages, data, payload, 1 | (2 if flags & 2 else 0) | (4 if flags & 1 else 0)))
    assert 32 + len(regions) * 40 <= PAGE
    struct.pack_into("<8sHBBIQQ", output, 0, b"SQBOOT01", 2, 12, 11, len(regions), entry, len(output))
    for i, region in enumerate(regions):
        struct.pack_into("<5Q", output, 32 + i * 40, *region)
    return struct.pack("<II", PAGE, len(output)).ljust(PAGE, b"\0") + output


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("kernel", type=Path, nargs="?", help="optional matching ABI 11 image")
    parser.add_argument("--profile", choices=["debug", "release"], default="release")
    parser.add_argument("--no-build", action="store_true")
    args = parser.parse_args()
    guest = Path(__file__).resolve().parent
    target = guest / "target"
    profile = ["--release"] if args.profile == "release" else []
    flags = "-Crelocation-model=static -Cforce-frame-pointers=yes -Ccode-model=medium -Dwarnings"
    if not args.no_build:
        subprocess.run([
            "cargo", "build", "--locked", *profile, "--target", TARGET,
            "--manifest-path", guest / "Cargo.toml", "--target-dir", target,
        ], cwd=guest, check=True, env={**os.environ, "RUSTFLAGS": flags})
        if not args.kernel:
            subprocess.run([
                "cargo", "+nightly-2026-11-14", "build", "--locked", *profile, "--target", TARGET,
                "--manifest-path", guest / "platform/Cargo.toml", "--target-dir", target,
            ], cwd=guest, check=True, env={**os.environ, "RUSTFLAGS": flags})
    binaries = target / TARGET / args.profile
    kernel = args.kernel.resolve(strict=True) if args.kernel else binaries / "twine-test-platform"
    elf = binaries / "twine-share-guest"
    evidence = target / "evidence" / args.profile
    evidence.mkdir(parents=True, exist_ok=True)
    image = evidence / "initrd.bin"
    image.write_bytes(initrd(elf.read_bytes()))
    firmware = Path(os.environ["QEMU_OPENSBI"]).resolve(strict=True)
    qemu = os.environ.get("QEMU_RISCV64", "qemu-system-riscv64")
    version = subprocess.check_output([qemu, "--version"], text=True).splitlines()[0]
    assert "version 10.0." in version, "This fixture is reviewed for QEMU 10.0.x"
    # Gamma qualifies local-Hart executable publication; SMP is not admitted.
    command = [qemu, "-accel", "tcg,thread=single", "-M", "virt", "-cpu", "rv64", "-m", "128M", "-smp", "1",
               "-bios", firmware, "-kernel", kernel, "-initrd", image,
               "-nographic", "-monitor", "none", "-nic", "none"]
    metadata = {"expected_kernel_tag": TAG, "built_kernel_sha": None if args.kernel else "9a35dd5420474ebe75a6b5f3e4589a398745e32c",
                "qemu": version, "command": [str(value) for value in command],
                "guest_rustc": subprocess.check_output(["rustc", "-Vv"], text=True),
                "platform_toolchain": "nightly-2026-11-14", "external_image": bool(args.kernel),
                "sha256": {str(p): hashlib.sha256(p.read_bytes()).hexdigest()
                           for p in [kernel, elf, image, firmware, guest / "Cargo.lock", guest / "platform/Cargo.lock"]}}
    (evidence / "inputs.json").write_text(json.dumps(metadata, indent=2))
    started = time.monotonic()
    with (evidence / "serial.log").open("wb") as serial:
        try:
            result = subprocess.run(command, stdout=serial, stderr=subprocess.STDOUT, timeout=60)
            status = result.returncode
        except subprocess.TimeoutExpired:
            status = "timeout"
    (evidence / "result.json").write_text(json.dumps({"status": status, "seconds": time.monotonic() - started}))
    output = (evidence / "serial.log").read_bytes()
    print(output.decode(errors="replace"), flush=True)
    assert status == 0 and PASS in output and b"FAIL twine-share" not in output, f"Guest failed: {evidence}"
    print(f"Evidence: {evidence}")


if __name__ == "__main__":
    main()
