# monitoring_engine

Native Rust extension module (built with [PyO3](https://pyo3.rs) +
[maturin](https://www.maturin.rs)) that offloads the hot, I/O- and
syscall-heavy parts of **Macan Monitoring** — disk scanning, byte
formatting, and temp-file cleanup — from Python to Rust for lower
overhead and no GIL contention during bulk operations.

Part of the [Macan ecosystem](https://github.com/danx123) suite of desktop apps.

## Why

Macan Monitoring polls disk usage and clears temp folders as part of its
normal UI operation. Two of those paths benefit from native code:

- **Bulk temp-file cleanup** — deleting thousands of small files/folders
  one-by-one via Python's `os.unlink()` / `shutil.rmtree()` pays GIL and
  interpreter overhead per item. Doing the whole walk-and-delete in Rust
  removes that overhead entirely.
- **Disk stat scanning / formatting** — cheap individually, but native
  calls avoid repeated Python-level object creation when polled on a
  timer.

The extension is designed to be **optional**: the Python side imports it
with a try/except and falls back to a pure-Python implementation if the
compiled wheel isn't present on a given machine. 

## Requirements

- Rust (stable toolchain)
- Python 3.10+ (built against `abi3-py310`, so one wheel works across
  3.10+)
- [maturin](https://www.maturin.rs) for building

## Building

```bash
pip install maturin
maturin build --release --out dist --find-interpreter
```

The resulting wheel in `dist/` can be installed with:

```bash
pip install dist/monitoring_engine-*.whl
```

For local development (editable install into your current venv):

```bash
maturin develop --release
```

CI (`.github/workflows/build.yml`) builds wheels for both **Windows x64**
and **Linux x64** on every push/PR to `main`.

## API

All functions live in the `monitoring_engine` module and are called
directly from Python — no async, no classes to instantiate.

### `scan_all_drives() -> list[DiskInfo]`

Scans all mounted drives and returns a list of `DiskInfo` objects.

```python
import monitoring_engine as engine

for disk in engine.scan_all_drives():
    print(disk.path, disk.name, disk.percent_used)
```

`DiskInfo` is a native PyO3 class (`#[pyclass]`) exposing:

| Field           | Type    | Description                          |
|-----------------|---------|---------------------------------------|
| `path`          | `str`   | Mount point (e.g. `C:\`, `/mnt/data`) |
| `name`          | `str`   | Volume/disk name                      |
| `total_bytes`   | `int`   | Total capacity in bytes               |
| `used_bytes`    | `int`   | Bytes used                            |
| `free_bytes`    | `int`   | Bytes free                            |
| `percent_used`  | `float` | Usage percentage (0–100)              |

> **Not currently wired into the Python UI.** `DiskWorker` in
> `macan_monitoring_new43.py` still uses `psutil` for its disk poll,
> because it also needs per-drive `device` path and `removable` flags for
> the eject-drive feature, which `sysinfo` doesn't expose consistently
> across platforms. This function is available for future use once that
> gap is addressed.

### `format_bytes(bytes: int) -> str`

Formats a byte count into a human-readable string with 2 decimal places.

```python
>>> engine.format_bytes(1_500_000_000)
"1.40 GB"
```

### `split_bytes(bytes: int) -> tuple[float, str]`

Same conversion as `format_bytes`, but returns the numeric value and unit
separately for custom formatting.

```python
>>> engine.split_bytes(1_500_000_000)
(1.396983861923218, "GB")
```

### `clear_temp_files(folder_path: str) -> tuple[int, int, int]`

Deletes every item directly inside `folder_path` — files removed with
`remove_file`, subfolders removed recursively with `remove_dir_all` —
and returns `(deleted, freed_bytes, failed)`.

```python
>>> engine.clear_temp_files(r"C:\Users\jenny\AppData\Local\Temp")
(842, 1073741824, 3)
# 842 items deleted, 1 GiB freed, 3 items skipped (in use / permission denied)
```

Folder sizes are computed **before** deletion (since they can't be read
afterward), so `freed_bytes` reflects everything removed, including the
contents of deleted subfolders — not just top-level files.

Used in the Python UI via `ClearTempWorker(QThread)`, which runs this off
the GUI thread so the "Clear Temp" button no longer freezes the app on
large temp folders.

### `get_system_ram() -> tuple[int, int]`

Returns `(total_bytes, used_bytes)` for system RAM.

```python
>>> engine.get_system_ram()
(17179869184, 8589934592)
```

> **Not currently wired into the Python UI.** `SystemMonitor` still uses
> `psutil.virtual_memory()`, whose `.percent` on Windows reads the OS's
> own `dwMemoryLoad` value directly — switching to this function would
> require recomputing that percentage from raw bytes, which can drift
> slightly from the OS-reported figure. Left available for future use.

### `get_drive_info(mount_path: str) -> tuple[int, int, int, float]`

Returns `(total, used, free, percent_used)` for a single drive by mount
path. Raises `ValueError` if the mount path isn't found.

```python
>>> engine.get_drive_info("C:\\")
(500107862016, 210453324800, 289654537216, 42.08)
```

## Fallback behavior

Every call site in the Python app checks `RUST_ENGINE_AVAILABLE` before
using this module, and catches exceptions around individual calls,
falling back to the equivalent pure-Python code path. The app runs
identically — just slower on bulk operations — on any machine where the
wheel hasn't been built or installed.

## Project layout

```
monitoring_engine/
├── Cargo.toml
├── src/
│   └── lib.rs
└── .github/workflows/build.yml
```


