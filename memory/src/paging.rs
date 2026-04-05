use core::{ptr::null_mut, sync::atomic::AtomicPtr};

use crate::addr::PhysAddr;

const ENTRIES_PER_TABLE: usize = 512;

#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct PageTableEntry(u64);

impl PageTableEntry {
    pub const PRESENT: u64 = 1 << 0;
    pub const WRITABLE: u64 = 1 << 1;
    pub const USER: u64 = 1 << 2;
    pub const WRITE_THROUGH: u64 = 1 << 3;
    pub const NO_CACHE: u64 = 1 << 4;
    pub const ACCESSED: u64 = 1 << 5;
    pub const DIRTY: u64 = 1 << 6;
    pub const HUGE: u64 = 1 << 7;
    pub const GLOBAL: u64 = 1 << 8;
    pub const NO_EXECUTE: u64 = 1 << 63;
    
    pub fn new() -> Self {
        PageTableEntry(0)
    }
    
    pub fn is_present(&self) -> bool {
        (self.0 & Self::PRESENT) != 0
    }
    
    pub fn set_addr(&mut self, addr: PhysAddr, flags: u64) {
        self.0 = (addr.as_u64() & 0x000F_FFFF_FFFF_F000) | flags;
    }
    
    pub fn get_addr(&self) -> PhysAddr {
        PhysAddr::new(self.0 & 0x000F_FFFF_FFFF_F000)
    }
    
    pub fn clear(&mut self) {
        self.0 = 0;
    }
}

#[repr(align(4096))]
pub struct PageTable {
    pub entries: [PageTableEntry; ENTRIES_PER_TABLE],
}

impl PageTable {
    pub fn new() -> Self {
        PageTable {
            entries: [PageTableEntry::new(); ENTRIES_PER_TABLE],
        }
    }
    
    pub fn zero(&mut self) {
        for entry in self.entries.iter_mut() {
            entry.clear();
        }
    }
}

pub static KERNEL_PAGE_TABLE: AtomicPtr<PageTable> = AtomicPtr::new(null_mut());