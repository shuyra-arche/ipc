"""Boot the Twine integration guest with a built Shuyra ABI 8 QEMU image."""

import argparse
import os
from pathlib import Path
import struct
import subprocess
import tempfile

PAGE = 4096
PASS = b"PASS twine-share teams=2 wrap=1 drain=1 revoke=1 exit=1"


def initrd(elf: bytes) -> bytes:
    # The guest linker script emits one RX segment containing code and constants.
    assert elf[:6] == b"\x7fELF\x02\x01"
    entry, table = struct.unpack_from("<QQ", elf, 24)
    width, count = struct.unpack_from("<HH", elf, 54)
    headers = [struct.unpack_from("<IIQQQQQQ", elf, table + i * width) for i in range(count)]
    [(kind, flags, offset, address, _, length, size, _)] = [h for h in headers if h[0] == 1]
    assert kind == 1 and flags == 5 and length == size and address % PAGE == 0
    pages = (size + PAGE - 1) // PAGE
    total = (1 + pages) * PAGE
    header = struct.pack("<8sHBBIQQ", b"SQBOOT01", 2, 12, 8, 1, entry, total)
    region = struct.pack("<QQQQQ", address, pages, pages, PAGE, 5)
    return (
        struct.pack("<II", PAGE, total).ljust(PAGE, b"\0")
        + (header + region).ljust(PAGE, b"\0")
        + elf[offset : offset + length].ljust(pages * PAGE, b"\0")
    )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("kernel", type=Path, help="Shuyra RV64 virt boot image")
    args = parser.parse_args()
    kernel = args.kernel.resolve(strict=True)
    guest = Path(__file__).resolve().parent
    qemu = os.environ.get("QEMU_RISCV64", "qemu-system-riscv64")
    bios = os.environ.get("QEMU_OPENSBI", "default")
    with tempfile.TemporaryDirectory(prefix="twine-qemu-") as directory:
        target = Path(directory)
        triple = "riscv64gc-unknown-none-elf"
        subprocess.run(
            [
                "cargo", "build", "--locked", "--release", "--target", triple,
                "--manifest-path", guest / "Cargo.toml", "--target-dir", target,
            ],
            cwd=guest,
            check=True,
        )
        image = target / "initrd"
        image.write_bytes(initrd((target / triple / "release/twine-share-guest").read_bytes()))
        for harts in ("1", "4"):
            command = [
                qemu, "-M", "virt", "-cpu", "rv64", "-m", "128M", "-smp", harts,
                "-bios", bios, "-kernel", kernel, "-initrd", image,
                "-nographic", "-monitor", "none", "-nic", "none",
            ]
            try:
                output = subprocess.run(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=60)
            except subprocess.TimeoutExpired as error:
                print((error.stdout or b"").decode(errors="replace"), flush=True)
                raise SystemExit(f"QEMU timed out with {harts} hart(s)") from error
            print(f"QEMU {harts} hart(s)\n{output.stdout.decode(errors='replace')}", flush=True)
            output.check_returncode()
            if PASS not in output.stdout or b"FAIL twine-share" in output.stdout:
                raise SystemExit(f"guest assertions failed with {harts} hart(s)")


if __name__ == "__main__":
    main()
