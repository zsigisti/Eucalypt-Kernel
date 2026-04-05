use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};
use framebuffer::println;

use crate::thread::{TCB, ThreadState, get_thread, get_thread_count};

pub static CURRENT_THREAD: AtomicPtr<TCB> = AtomicPtr::new(core::ptr::null_mut());
static CURRENT_INDEX: AtomicUsize = AtomicUsize::new(0);

pub fn schedule(old_rsp: u64) -> u64 {
    let count = get_thread_count();
    if count < 2 {
        println!("schedule: only {} thread(s), not switching", count);
        return old_rsp;
    }

    let old_index = CURRENT_INDEX.load(Ordering::Acquire);
    let old = get_thread(old_index);

    unsafe { (*old).cpu_context.rsp = old_rsp; }

    let new_index = (old_index + 1) % count;
    let new = get_thread(new_index);

    println!("schedule: {} -> {} (new rsp=0x{:X})", old_index, new_index, unsafe { (*new).cpu_context.rsp });

    CURRENT_INDEX.store(new_index, Ordering::Release);
    CURRENT_THREAD.store(new, Ordering::Release);

    unsafe {
        let new_cr3 = (*new).cr3;
        let old_cr3: u64;
        core::arch::asm!("mov {}, cr3", out(reg) old_cr3);
        if old_cr3 != new_cr3 {
            core::arch::asm!("mov cr3, {}", in(reg) new_cr3);
        }
        (*new).cpu_context.rsp
    }
}

pub fn get_current_thread() -> *mut TCB {
    CURRENT_THREAD.load(Ordering::Acquire)
}

pub fn set_current_thread(tcb: *mut TCB) {
    CURRENT_THREAD.store(tcb, Ordering::Release);
}

pub fn set_current_index(index: usize) {
    CURRENT_INDEX.store(index, Ordering::Release);
}

pub fn get_current_index() -> usize {
    CURRENT_INDEX.load(Ordering::Acquire)
}