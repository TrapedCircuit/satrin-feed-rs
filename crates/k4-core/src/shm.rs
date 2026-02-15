//! Shared memory ring-buffer store for market data.
//!
//! Provides a generic `ShmMdStore<T>` that maps a POSIX shared memory region
//! and organises it as a per-symbol circular buffer. This is the primary
//! mechanism for distributing market data to downstream consumers (strategies,
//! loggers, etc.) with zero-copy reads.
//!
//! # Memory layout
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │ ShmHeader (update_num, instrument_count, buffer_size)       │
//! ├─────────────────────────────────────────────────────────────┤
//! │ InstrumentSlot[0]: InstrumentHeader + T[buffer_size]        │
//! │ InstrumentSlot[1]: InstrumentHeader + T[buffer_size]        │
//! │ ...                                                         │
//! │ InstrumentSlot[N-1]                                         │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! The `current_index` in each `InstrumentHeader` is atomically incremented on
//! each write, and readers use it to locate the latest sample.

use std::{
    collections::HashMap,
    sync::atomic::{AtomicI64, Ordering},
};

use crate::types::symbol::{SYMBOL_LEN, symbol_to_bytes};

// ---------------------------------------------------------------------------
// On-disk (mmap) structures
// ---------------------------------------------------------------------------

/// Global header at the start of the shared memory region.
#[repr(C)]
pub struct ShmHeader {
    /// Total number of updates written across all instruments.
    pub update_num: u64,
    /// Number of instruments in this SHM region.
    pub instrument_count: u32,
    /// Ring buffer size per instrument (number of `T` slots).
    pub buffer_size: u32,
}

/// Per-instrument header preceding its ring buffer.
#[repr(C)]
pub struct InstrumentHeader {
    /// Symbol name, null-padded.
    pub symbol: [u8; SYMBOL_LEN],
    /// Current write index (atomically updated). Readers should load with
    /// `Acquire` ordering; writers store with `Release`.
    pub current_index: AtomicI64,
    /// Number of `T` slots in this instrument's buffer.
    pub buffer_len: u32,
    /// Padding for alignment.
    _pad: u32,
}

// ---------------------------------------------------------------------------
// ShmMdStore
// ---------------------------------------------------------------------------

/// A typed shared-memory market data store.
///
/// Manages a memory-mapped region containing per-symbol ring buffers of type `T`.
/// Supports both writer (create) and reader (open) modes.
pub struct ShmMdStore<T: Copy> {
    /// Base pointer to the mmap'd region.
    #[allow(dead_code)]
    base: *mut u8,
    /// Total size of the mmap'd region in bytes.
    #[allow(dead_code)]
    total_size: usize,
    /// Ring buffer capacity per instrument.
    buffer_size: u32,
    /// Map from symbol string to (InstrumentHeader ptr, data slice base ptr).
    index: HashMap<String, (*mut InstrumentHeader, *mut T)>,
    /// SHM name (for cleanup).
    #[allow(dead_code)]
    shm_name: String,
}

// SAFETY: The pointers point to mmap'd memory that outlives the struct.
// Access is synchronized via atomic current_index for single-writer use.
unsafe impl<T: Copy> Send for ShmMdStore<T> {}
unsafe impl<T: Copy> Sync for ShmMdStore<T> {}

impl<T: Copy> ShmMdStore<T> {
    /// Calculate the total mmap size needed for the given parameters.
    fn calc_size(instrument_count: usize, buffer_size: u32) -> usize {
        let header_size = std::mem::size_of::<ShmHeader>();
        let slot_size = std::mem::size_of::<InstrumentHeader>() + std::mem::size_of::<T>() * buffer_size as usize;
        header_size + slot_size * instrument_count
    }

    /// Create a new shared memory region and initialize it for writing.
    ///
    /// # Arguments
    /// - `shm_name`: POSIX shared memory name (e.g. `"spot_bbo"`)
    /// - `symbols`: list of instrument symbols to allocate slots for
    /// - `buffer_size`: number of `T` entries per symbol ring buffer
    #[cfg(target_os = "linux")]
    pub fn create(shm_name: &str, symbols: &[String], buffer_size: u32) -> anyhow::Result<Self> {
        use std::ffi::CString;

        let instrument_count = symbols.len();
        let total_size = Self::calc_size(instrument_count, buffer_size);

        // Remove stale SHM if it exists
        let shm_path = format!("/dev/shm/{shm_name}");
        let _ = std::fs::remove_file(&shm_path);

        let c_name = CString::new(shm_name)?;

        // SAFETY: POSIX shm_open + ftruncate + mmap — standard IPC pattern.
        unsafe {
            let fd = libc::shm_open(c_name.as_ptr(), libc::O_CREAT | libc::O_RDWR, 0o666);
            if fd < 0 {
                return Err(anyhow::anyhow!("shm_open failed: {}", std::io::Error::last_os_error()));
            }

            if libc::ftruncate(fd, total_size as libc::off_t) != 0 {
                libc::close(fd);
                return Err(anyhow::anyhow!("ftruncate failed"));
            }

            let base = libc::mmap(
                std::ptr::null_mut(),
                total_size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            );
            libc::close(fd);

            if base == libc::MAP_FAILED {
                return Err(anyhow::anyhow!("mmap failed"));
            }

            // Zero-initialize
            std::ptr::write_bytes(base as *mut u8, 0, total_size);

            let base = base as *mut u8;

            // Write global header
            let header = &mut *(base as *mut ShmHeader);
            header.update_num = 0;
            header.instrument_count = instrument_count as u32;
            header.buffer_size = buffer_size;

            // Initialize instrument slots and build index
            let mut index = HashMap::new();
            let mut offset = std::mem::size_of::<ShmHeader>();

            for sym in symbols {
                let inst_hdr = &mut *(base.add(offset) as *mut InstrumentHeader);
                inst_hdr.symbol = symbol_to_bytes(sym);
                inst_hdr.current_index = AtomicI64::new(-1);
                inst_hdr.buffer_len = buffer_size;

                let data_ptr = base.add(offset + std::mem::size_of::<InstrumentHeader>()) as *mut T;

                index.insert(sym.clone(), (inst_hdr as *mut InstrumentHeader, data_ptr));

                offset += std::mem::size_of::<InstrumentHeader>() + std::mem::size_of::<T>() * buffer_size as usize;
            }

            Ok(Self { base, total_size, buffer_size, index, shm_name: shm_name.to_string() })
        }
    }

    /// Stub for non-Linux platforms (shared memory is Linux-only in production).
    #[cfg(not(target_os = "linux"))]
    pub fn create(shm_name: &str, symbols: &[String], buffer_size: u32) -> anyhow::Result<Self> {
        // On macOS/Windows, allocate a heap buffer to allow development/testing.
        let instrument_count = symbols.len();
        let total_size = Self::calc_size(instrument_count, buffer_size);

        let layout =
            std::alloc::Layout::from_size_align(total_size, 64).map_err(|e| anyhow::anyhow!("layout error: {e}"))?;

        // SAFETY: we immediately zero-initialize the allocation.
        let base = unsafe {
            let ptr = std::alloc::alloc_zeroed(layout);
            if ptr.is_null() {
                return Err(anyhow::anyhow!("allocation failed"));
            }
            ptr
        };

        unsafe {
            let header = &mut *(base as *mut ShmHeader);
            header.update_num = 0;
            header.instrument_count = instrument_count as u32;
            header.buffer_size = buffer_size;

            let mut index = HashMap::new();
            let mut offset = std::mem::size_of::<ShmHeader>();

            for sym in symbols {
                let inst_hdr = &mut *(base.add(offset) as *mut InstrumentHeader);
                inst_hdr.symbol = symbol_to_bytes(sym);
                inst_hdr.current_index = AtomicI64::new(-1);
                inst_hdr.buffer_len = buffer_size;

                let data_ptr = base.add(offset + std::mem::size_of::<InstrumentHeader>()) as *mut T;
                index.insert(sym.clone(), (inst_hdr as *mut InstrumentHeader, data_ptr));

                offset += std::mem::size_of::<InstrumentHeader>() + std::mem::size_of::<T>() * buffer_size as usize;
            }

            Ok(Self { base, total_size, buffer_size, index, shm_name: shm_name.to_string() })
        }
    }

    /// Write a new data point for the given symbol.
    ///
    /// The write index is atomically incremented so concurrent readers always
    /// see a consistent snapshot.
    #[inline]
    pub fn write(&self, symbol: &str, data: &T) -> bool {
        if let Some(&(hdr, data_base)) = self.index.get(symbol) {
            unsafe {
                let hdr = &*hdr;
                let next = hdr.current_index.load(Ordering::Relaxed) + 1;
                let slot = (next as u64 % self.buffer_size as u64) as usize;

                // Write data to the slot
                std::ptr::write(data_base.add(slot), *data);

                // Publish the new index with Release ordering so readers see the data
                hdr.current_index.store(next, Ordering::Release);
            }
            true
        } else {
            false
        }
    }

    /// Read the latest data point for the given symbol.
    ///
    /// Returns `None` if the symbol is not found or no data has been written.
    #[inline]
    pub fn read_latest(&self, symbol: &str) -> Option<T> {
        let &(hdr, data_base) = self.index.get(symbol)?;
        unsafe {
            let hdr = &*hdr;
            let idx = hdr.current_index.load(Ordering::Acquire);
            if idx < 0 {
                return None;
            }
            let slot = (idx as u64 % self.buffer_size as u64) as usize;
            Some(std::ptr::read(data_base.add(slot)))
        }
    }

    /// Returns the list of symbols in this store.
    pub fn symbols(&self) -> Vec<String> {
        self.index.keys().cloned().collect()
    }

    /// Check if a symbol exists in this store.
    pub fn contains_symbol(&self, symbol: &str) -> bool {
        self.index.contains_key(symbol)
    }
}

impl<T: Copy> Drop for ShmMdStore<T> {
    fn drop(&mut self) {
        // On Linux, we should munmap. On non-Linux dev builds, dealloc.
        #[cfg(target_os = "linux")]
        unsafe {
            libc::munmap(self.base as *mut libc::c_void, self.total_size);
            // Note: we don't shm_unlink here — the SHM persists for readers.
        }

        #[cfg(not(target_os = "linux"))]
        unsafe {
            if !self.base.is_null()
                && let Ok(layout) = std::alloc::Layout::from_size_align(self.total_size, 64)
            {
                std::alloc::dealloc(self.base, layout);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_and_read() {
        let symbols = vec!["BTCUSDT".to_string(), "ETHUSDT".to_string()];
        let store = ShmMdStore::<u64>::create("test_shm_wr", &symbols, 100).unwrap();

        // Initially no data
        assert!(store.read_latest("BTCUSDT").is_none());

        // Write and read back
        store.write("BTCUSDT", &42);
        assert_eq!(store.read_latest("BTCUSDT"), Some(42));

        // Overwrite
        store.write("BTCUSDT", &99);
        assert_eq!(store.read_latest("BTCUSDT"), Some(99));

        // Other symbol unaffected
        assert!(store.read_latest("ETHUSDT").is_none());
    }

    #[test]
    fn unknown_symbol() {
        let store = ShmMdStore::<u64>::create("test_shm_unk", &[], 100).unwrap();
        assert!(!store.write("UNKNOWN", &1));
        assert!(store.read_latest("UNKNOWN").is_none());
    }

    #[test]
    fn ring_buffer_wraparound() {
        let symbols = vec!["BTCUSDT".to_string()];
        let store = ShmMdStore::<u64>::create("test_shm_wrap", &symbols, 4).unwrap();
        // Write more than buffer_size entries
        for i in 0u64..10 {
            store.write("BTCUSDT", &i);
        }
        // Latest should be the last written value
        assert_eq!(store.read_latest("BTCUSDT"), Some(9));
    }
}
