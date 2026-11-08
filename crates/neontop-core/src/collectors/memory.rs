use serde::Serialize;
use std::fs;

/// Valores de /proc/meminfo en bytes. El reparto (apps/caché/buffers…) se calcula en el frontend.
#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct MemSnapshot {
    pub total: u64,
    pub free: u64,
    pub available: u64,
    pub buffers: u64,
    pub cached: u64,
    pub s_reclaimable: u64,
    pub shmem: u64,
    pub swap_total: u64,
    pub swap_free: u64,
    pub swap_cached: u64,
    pub dirty: u64,
    pub writeback: u64,
    pub active: u64,
    pub inactive: u64,
    pub active_anon: u64,
    pub inactive_anon: u64,
    pub active_file: u64,
    pub inactive_file: u64,
    pub slab: u64,
    pub kernel_stack: u64,
    pub page_tables: u64,
    pub mapped: u64,
    pub anon_pages: u64,
    pub zswap: u64,
    pub zswapped: u64,
    pub committed_as: u64,
    pub commit_limit: u64,
}

pub fn sample() -> MemSnapshot {
    let mut m = MemSnapshot::default();
    let Ok(text) = fs::read_to_string("/proc/meminfo") else { return m };
    for line in text.lines() {
        let Some((key, rest)) = line.split_once(':') else { continue };
        let kb: u64 = rest.split_whitespace().next().and_then(|v| v.parse().ok()).unwrap_or(0);
        let bytes = kb * 1024;
        let slot = match key {
            "MemTotal" => &mut m.total,
            "MemFree" => &mut m.free,
            "MemAvailable" => &mut m.available,
            "Buffers" => &mut m.buffers,
            "Cached" => &mut m.cached,
            "SReclaimable" => &mut m.s_reclaimable,
            "Shmem" => &mut m.shmem,
            "SwapTotal" => &mut m.swap_total,
            "SwapFree" => &mut m.swap_free,
            "SwapCached" => &mut m.swap_cached,
            "Dirty" => &mut m.dirty,
            "Writeback" => &mut m.writeback,
            "Active" => &mut m.active,
            "Inactive" => &mut m.inactive,
            "Active(anon)" => &mut m.active_anon,
            "Inactive(anon)" => &mut m.inactive_anon,
            "Active(file)" => &mut m.active_file,
            "Inactive(file)" => &mut m.inactive_file,
            "Slab" => &mut m.slab,
            "KernelStack" => &mut m.kernel_stack,
            "PageTables" => &mut m.page_tables,
            "Mapped" => &mut m.mapped,
            "AnonPages" => &mut m.anon_pages,
            "Zswap" => &mut m.zswap,
            "Zswapped" => &mut m.zswapped,
            "Committed_AS" => &mut m.committed_as,
            "CommitLimit" => &mut m.commit_limit,
            _ => continue,
        };
        *slot = bytes;
    }
    m
}
