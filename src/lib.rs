use pyo3::prelude::*;
use sysinfo::{System, Disk, Disks};
use ahash::AHashMap as HashMap;
use std::fs;
use std::path::Path;

// ═══════════════════════════════════════════════════════════
// MONITORING ENGINE — Pemindaian Disk, Info Sistem, Pembersihan
// ═══════════════════════════════════════════════════════════

// Add #[pyclass] to make it a Python object
#[pyclass]
#[derive(Debug, Clone)]
pub struct DiskInfo {
    #[pyo3(get)] // Exposes the property to Python
    pub path: String,
    
    #[pyo3(get)]
    pub name: String,
    
    #[pyo3(get)]
    pub total_bytes: u64,
    
    #[pyo3(get)]
    pub used_bytes: u64,
    
    #[pyo3(get)]
    pub free_bytes: u64,
    
    #[pyo3(get)]
    pub percent_used: f64,
}

// ═══════════════════════════════════════════════
// BAGIAN 1: PEMINDAIAN SEMUA DRIVE
// ═══════════════════════════════════════════════

#[pyfunction]
fn scan_all_drives() -> PyResult<Vec<DiskInfo>> {
    let disks = Disks::new_with_refreshed_list();
    let mut result = Vec::with_capacity(8);

    for disk in disks.list() {
        let mount_point = disk.mount_point().to_string_lossy().to_string();
        let name = disk.name().to_string_lossy().to_string();
        let total = disk.total_space();
        let free = disk.available_space();
        let used = total - free;
        let percent = if total > 0 { (used as f64 / total as f64) * 100.0 } else { 0.0 };

        result.push(DiskInfo {
            path: mount_point,
            name,
            total_bytes: total,
            used_bytes: used,
            free_bytes: free,
            percent_used: percent,
        });
    }

    Ok(result)
}

// ═══════════════════════════════════════════════
// BAGIAN 2: FORMAT SATUAN (Byte → KB/MB/GB/TB)
// ═══════════════════════════════════════════════

#[pyfunction]
fn format_bytes(bytes: u64) -> PyResult<String> {
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit_index = 0;

    while value >= 1024.0 && unit_index < 4 {
        value /= 1024.0;
        unit_index += 1;
    }

    Ok(format!("{:.2} {}", value, units[unit_index]))
}

#[pyfunction]
fn split_bytes(bytes: u64) -> PyResult<(f64, String)> {
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit_index = 0;

    while value >= 1024.0 && unit_index < 4 {
        value /= 1024.0;
        unit_index += 1;
    }

    Ok((value, units[unit_index].to_string()))
}

// ═══════════════════════════════════════════════
// BAGIAN 3: PEMBERSIHAN FILE SEMENTARA
// ═══════════════════════════════════════════════

/// Hitung total ukuran sebuah folder secara rekursif (dipakai untuk statistik
/// freed_bytes sebelum folder itu dihapus — ukurannya nggak bisa dibaca lagi
/// setelah dihapus).
fn dir_size_recursive(path: &Path) -> u64 {
    let mut total = 0u64;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let p = entry.path();
            if let Ok(md) = entry.metadata() {
                if md.is_dir() {
                    total += dir_size_recursive(&p);
                } else {
                    total += md.len();
                }
            }
        }
    }
    total
}

#[pyfunction]
fn clear_temp_files(folder_path: &str) -> PyResult<(u32, u64, u32)> {
    // → Mengembalikan (item_dihapus, total_byte_dibersihkan, item_gagal)
    // Menghapus SEMUA item langsung di dalam folder_path (file maupun
    // subfolder, dihapus rekursif) — setara `os.unlink` + `shutil.rmtree`
    // versi Python-nya, tapi jalan native tanpa GIL.
    let path = Path::new(folder_path);
    if !path.exists() {
        return Ok((0, 0, 0));
    }

    let mut deleted = 0u32;
    let mut failed = 0u32;
    let mut freed_bytes = 0u64;

    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let p = entry.path();
            let md = match entry.metadata() {
                Ok(m) => m,
                Err(_) => {
                    failed += 1;
                    continue;
                }
            };

            if md.is_dir() {
                let size = dir_size_recursive(&p);
                match fs::remove_dir_all(&p) {
                    Ok(_) => {
                        deleted += 1;
                        freed_bytes += size;
                    }
                    Err(_) => failed += 1,
                }
            } else {
                // Menutupi file biasa maupun symlink.
                let size = md.len();
                match fs::remove_file(&p) {
                    Ok(_) => {
                        deleted += 1;
                        freed_bytes += size;
                    }
                    Err(_) => failed += 1,
                }
            }
        }
    }

    Ok((deleted, freed_bytes, failed))
}

// ═══════════════════════════════════════════════
// BAGIAN 4: INFORMASI SISTEM (RAM)
// ═══════════════════════════════════════════════

#[pyfunction]
fn get_system_ram() -> PyResult<(u64, u64)> {
    // → (total_bytes, dipakai_bytes)
    let mut sys = System::new();
    sys.refresh_memory();
    Ok((sys.total_memory(), sys.used_memory()))
}

// ═══════════════════════════════════════════════
// BAGIAN 5: INFORMASI PER DRIVE SATUAN
// ═══════════════════════════════════════════════

#[pyfunction]
fn get_drive_info(mount_path: &str) -> PyResult<(u64, u64, u64, f64)> {
    // → (total, digunakan, tersedia, persen_terpakai)
    let disks = Disks::new_with_refreshed_list();

    for disk in disks.list() {
        if disk.mount_point().to_string_lossy() == mount_path {
            let total = disk.total_space();
            let free = disk.available_space();
            let used = total - free;
            let percent = if total > 0 { (used as f64 / total as f64) * 100.0 } else { 0.0 };
            return Ok((total, used, free, percent));
        }
    }

    Err(PyErr::new::<pyo3::exceptions::PyValueError, _>(
        format!("Drive tidak ditemukan: {}", mount_path)
    ))
}

// ═══════════════════════════════════════════════
// DAFTAR FUNGSI KE PYTHON
// ═══════════════════════════════════════════════

#[pymodule]
fn monitoring_engine(_py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(scan_all_drives, m)?)?;
    m.add_function(wrap_pyfunction!(format_bytes, m)?)?;
    m.add_function(wrap_pyfunction!(split_bytes, m)?)?;
    m.add_function(wrap_pyfunction!(clear_temp_files, m)?)?;
    m.add_function(wrap_pyfunction!(get_system_ram, m)?)?;
    m.add_function(wrap_pyfunction!(get_drive_info, m)?)?;
    Ok(())
}
