use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};
use crate::thread::{TCB, ThreadState, get_thread, get_thread_count};

pub static CURRENT_THREAD: AtomicPtr<TCB> = AtomicPtr::new(core::ptr::null_mut());
static CURRENT_INDEX: AtomicUsize = AtomicUsize::new(0);
static TICK_COUNT: AtomicUsize = AtomicUsize::new(0);
const TICKS_PER_SLICE: usize = 2;

pub fn schedule(old_rsp: u64) -> u64 {
    let count = get_thread_count();
    if count < 2 {
        return old_rsp;
    }

    let old_index = CURRENT_INDEX.load(Ordering::Acquire);
    let old = get_thread(old_index);

    unsafe {
        if (*old).state == ThreadState::Running {
            (*old).state = ThreadState::Ready;
        }
        (*old).cpu_context.rsp = old_rsp;
    }

    let ticks = TICK_COUNT.fetch_add(1, Ordering::AcqRel);
    if ticks % TICKS_PER_SLICE != 0 {
        return old_rsp;
    }

    let mut next_index = (old_index + 1) % count;
    let mut new_ptr = get_thread(next_index);

    unsafe {
        let mut iterations = 0usize;
        loop {
            let state = (*new_ptr).state;
            let rsp = (*new_ptr).cpu_context.rsp;

            if state != ThreadState::Blocked
                && state != ThreadState::Dead
                && state != ThreadState::Zombie
                && rsp != 0
            {
                break;
            }

            next_index = (next_index + 1) % count;
            iterations += 1;
            if iterations >= count {
                (*old).state = ThreadState::Running;
                return old_rsp;
            }
            new_ptr = get_thread(next_index);
        }

        if next_index == old_index {
            (*old).state = ThreadState::Running;
            return old_rsp;
        }

        (*new_ptr).state = ThreadState::Running;
        CURRENT_INDEX.store(next_index, Ordering::Release);
        CURRENT_THREAD.store(new_ptr, Ordering::Release);

        let new_cr3 = (*new_ptr).cr3;
        let mut current_cr3: u64;
        core::arch::asm!("mov {}, cr3", out(reg) current_cr3);
        if new_cr3 != 0 && current_cr3 != new_cr3 {
            core::arch::asm!("mov cr3, {}", in(reg) new_cr3);
        }

        let new_rsp = (*new_ptr).cpu_context.rsp;
        assert!(new_rsp != 0, "schedule: new thread {} has rsp=0", (*new_ptr).tid);
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