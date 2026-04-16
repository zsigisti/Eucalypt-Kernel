#![no_std]

extern crate alloc;

use core::ptr;
use framebuffer::println;
use memory;

static mut TTY_VIRT_ADDR: u64 = 0;

#[repr(C)]
struct TtyBuffer {
    write_ptr: u32,
    read_ptr: u32,
    data: [u8; 4088],
}

pub fn tty_init() {
    let phys = memory::frame_allocator::FrameAllocator::alloc_frame().expect("Failed to allocate frame");
    let pml4 = memory::vmm::VMM::get_kernel_mapper().create_user_pml4().expect("PML4 creation failed");
    let virt = memory::mmio::map_mmio(pml4, phys.as_u64(), 0x1000).expect("Mapping failed");
    
    unsafe {
        TTY_VIRT_ADDR = virt;
        let tty = &mut *(TTY_VIRT_ADDR as *mut TtyBuffer);
        ptr::write_volatile(&mut tty.write_ptr, 0);
        ptr::write_volatile(&mut tty.read_ptr, 0);
    }
}

pub fn tty_write(data: &[u8]) {
    for &byte in data {
        tty_write_byte(byte);
    }
}

pub fn tty_read(buf: &mut [u8]) {
    for i in 0..buf.len() {
        buf[i] = tty_read_byte();
    }
}

pub fn tty_write_byte(byte: u8) {
    unsafe {
        if TTY_VIRT_ADDR == 0 { return; }
        let tty = &mut *(TTY_VIRT_ADDR as *mut TtyBuffer);
        let index = (ptr::read_volatile(&tty.write_ptr) as usize) % tty.data.len();
        ptr::write_volatile(&mut tty.data[index], byte);
        ptr::write_volatile(&mut tty.write_ptr, tty.write_ptr.wrapping_add(1));
        core::arch::asm!("int 0x40");
    }
}

pub fn tty_read_byte() -> u8 {
    unsafe {
        if TTY_VIRT_ADDR == 0 { return 0; }
        let tty = &mut *(TTY_VIRT_ADDR as *mut TtyBuffer);
        while ptr::read_volatile(&tty.write_ptr) == ptr::read_volatile(&tty.read_ptr) {
            core::hint::spin_loop();
        }
        let index = (ptr::read_volatile(&tty.read_ptr) as usize) % tty.data.len();
        let byte = ptr::read_volatile(&tty.data[index]);
        ptr::write_volatile(&mut tty.read_ptr, tty.read_ptr.wrapping_add(1));
        byte
    }
}