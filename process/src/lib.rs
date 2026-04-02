#![no_std]

/// Creation yes and destruction yes
extern crate alloc;

pub mod proc;
pub mod threads;

use alloc::vec::Vec;
use memory::paging::PageTable;
use vfs::FD;

pub use proc::*;

pub struct TCB {
    pub tid: u64,
    pub rsp: u64,
    pub stack_base: *mut u8,
    pub entry: *mut (),
}

pub struct Process {
    pub pid: u64,
    pub rsp: u64,
    pub threads: Vec<TCB>,
    pub stack_base: *mut u8,
    pub entry: *mut (),
    pub pml4: *mut PageTable,
    pub state: ProcessState,
    pub priority: Priority,
    pub fildes: [FD; 1024], 
    pub ticks_ready: u64,
    pub wake_at_tick: u64,
}