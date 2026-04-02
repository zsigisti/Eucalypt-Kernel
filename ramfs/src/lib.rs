#![no_std]

extern crate alloc;
use alloc::boxed::Box;

use limine::request::ModulesResponse;
use framebuffer::println;
use vfs::{vfs_mount, RamFs};

pub fn init_ramdisk(module_response: &ModulesResponse) -> (usize, u64) {
    if module_response.modules().iter().count() < 1 {
        panic!("Modules ramfs not found");
    }

    let module = module_response.modules()[0];
    let module_addr = module.data().as_ptr() as usize;
    let module_size = module.data().len() as u64 - 1;
    
    // Check if it's at least a floppy image size
    if module_size < 1474560 {
        panic!("Wrong module size: expected >= 1474560, got {}", module_size);
    }
    
    println!("Ramdisk Address: {:?}, Size: {}", module_addr, module_size);
    (module_addr, module_size)
}

/// Initialises the ramdisk from Limine and mounts it into the VFS.
pub fn mount_ramdisk(module_response: &ModulesResponse, mount_point: &'static str) -> Result<(), &'static str> {
    let (addr, size) = init_ramdisk(module_response);
    
    let ramfs = RamFs::new();
    ramfs.load_from_fat12(addr as *mut u8, size)?;
    
    vfs_mount(mount_point, Box::new(ramfs))?;
    println!("Ramdisk mounted at /{}", mount_point);
    
    Ok(())
}