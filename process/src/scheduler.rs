use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering};
use crate::thread::{TCB, ThreadState, get_thread, get_thread_count};
use gdt::write_tss_rsp0;

pub static CURRENT_THREAD: AtomicPtr<TCB> = AtomicPtr::new(core::ptr::null_mut());
static CURRENT_INDEX: AtomicUsize = AtomicUsize::new(0);
static TICK_COUNT: AtomicUsize = AtomicUsize::new(0);
static ENABLED: AtomicBool = AtomicBool::new(false);
const TICKS_PER_SLICE: usize = 10;

pub fn enable_scheduler() {
    ENABLED.store(true, Ordering::Release);
}

pub fn disable_scheduler() {
    ENABLED.store(false, Ordering::Release);
}

pub fn schedule(old_rsp: u64) -> u64 {
    if !ENABLED.load(Ordering::Acquire) {
        return old_rsp;
    }

    let count = get_thread_count();
    if count < 2 {
        return old_rsp;
    }

    let old_index = CURRENT_INDEX.load(Ordering::Acquire);
    let old_tcb_ptr = get_thread(old_index);

    unsafe {
        if (*old_tcb_ptr).state == ThreadState::Running {
            (*old_tcb_ptr).state = ThreadState::Ready;
        }
        (*old_tcb_ptr).cpu_context.rsp = old_rsp;
    }

    let ticks = TICK_COUNT.fetch_add(1, Ordering::AcqRel);
    if (ticks + 1) % TICKS_PER_SLICE != 0 {
        unsafe {
            (*old_tcb_ptr).state = ThreadState::Running;
        }
        return old_rsp;
    }

    let mut next_index = (old_index + 1) % count;

    unsafe {
        let mut iterations = 0;
        loop {
            let next_tcb_ptr = get_thread(next_index);
            let state = (*next_tcb_ptr).state;
            let rsp = (*next_tcb_ptr).cpu_context.rsp;

            if state == ThreadState::Ready && rsp != 0 {
                break;
            }

            next_index = (next_index + 1) % count;
            iterations += 1;

            if iterations >= count {
                (*old_tcb_ptr).state = ThreadState::Running;
                return old_rsp;
            }
        }

        let new_tcb_ptr = get_thread(next_index);

        (*new_tcb_ptr).state = ThreadState::Running;
        CURRENT_INDEX.store(next_index, Ordering::Release);
        CURRENT_THREAD.store(new_tcb_ptr, Ordering::Release);

        if (*new_tcb_ptr).is_userspace && (*new_tcb_ptr).kernel_stack_top != 0 {
            write_tss_rsp0((*new_tcb_ptr).kernel_stack_top);
        }

        let new_cr3 = (*new_tcb_ptr).cr3;
        if new_cr3 != 0 {
            let mut current_cr3: u64;
            core::arch::asm!("mov {}, cr3", out(reg) current_cr3);
            if current_cr3 != new_cr3 {
                core::arch::asm!("mov cr3, {}", in(reg) new_cr3);
            }
        }

        let new_rsp = (*new_tcb_ptr).cpu_context.rsp;
        assert!(new_rsp != 0, "Scheduler Error: Thread {} has NULL RSP", (*new_tcb_ptr).tid);

        new_rsp
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

pub fn yield_() {
    unsafe {
        let current = get_current_thread();
        let next_rsp = schedule((*current).cpu_context.rsp);

        unsafe extern "C" {
            unsafe fn context_switch(old_rsp: *mut u64, new_rsp: u64);
        }

        context_switch(&mut (*current).cpu_context.rsp, next_rsp);
    }
}