use limine::memmap::MEMMAP_USABLE;
use limine::request::MemmapResponse;

use super::addr::PhysAddr;

const PAGE_SIZE: u64 = 0x1000;

static mut PAGE_LIST: usize = 0;

fn frame_alloc() -> usize {
    unsafe {
        let page = PAGE_LIST;

        if page == 0 {
            panic!("PMM: Out of memory");
        }

        let next_ptr = crate::hhdm::phys_to_virt(page) as *mut usize;
        PAGE_LIST = *next_ptr;

        page
    }
}

fn frame_free(addr: usize) {
    if addr % PAGE_SIZE as usize != 0 {
        panic!("PMM: Attempted to free unaligned frame: {:#x}", addr);
    }

    unsafe {
        let next_ptr = crate::hhdm::phys_to_virt(addr) as *mut usize;
        
        *next_ptr = PAGE_LIST;

        PAGE_LIST = addr;
    }
}

pub struct FrameAllocator;

impl FrameAllocator {
    pub fn alloc_frame() -> Option<PhysAddr> {
        let addr = frame_alloc();
        if addr == 0 {
            None
        } else {
            Some(PhysAddr::new(addr as u64))
        }
    }

    pub fn free_frame(frame: PhysAddr) {
        frame_free(frame.as_u64() as usize);
    }
}

pub fn init_frame_allocator(memmap_response: &MemmapResponse) {
    for entry in memmap_response.entries() {
        if entry.type_ != MEMMAP_USABLE {
            continue;
        }

        let mut current_addr = entry.base;
        let end_addr = entry.base + entry.length;

        while current_addr < end_addr {
            frame_free(current_addr as usize);
            current_addr += PAGE_SIZE;
        }
    }
}