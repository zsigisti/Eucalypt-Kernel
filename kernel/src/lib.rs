#![no_std]
#![feature(abi_x86_interrupt)]

extern crate alloc;

// Modules
pub mod gdt;
pub mod idt;
pub mod elf;
pub mod mp;

// C functions go here
unsafe extern "C" {
}