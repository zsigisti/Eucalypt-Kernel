use crate::thread::{TCB, ThreadState, get_thread, get_thread_count};
use crate::proc::{self, ProcessState};
use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering};
use gdt::write_tss_rsp0;

pub static CURRENT_THREAD: AtomicPtr<TCB> = AtomicPtr::new(core::ptr::null_mut());
static CURRENT_INDEX: AtomicUsize = AtomicUsize::new(0);
static TICK_COUNT: AtomicUsize = AtomicUsize::new(0);
static ENABLED: AtomicBool = AtomicBool::new(false);

const TICKS_PER_SLICE: usize = 5;

/// Allows the scheduler to switch threads on the next timer tick.
pub fn enable_scheduler() {
    ENABLED.store(true, Ordering::Release);
}

/// Prevents the scheduler from switching threads; safe to call from panic handlers.
pub fn disable_scheduler() {
    ENABLED.store(false, Ordering::Release);
}

/// Returns the PID of the currently running thread's owning process, or 0 if none.
pub fn get_current_pid() -> u64 {
    let ptr = get_current_thread();
    if ptr.is_null() {
        0
    } else {
        unsafe { (*ptr).pid }
    }
}

/// Calls `f` with a shared reference to the current process's PCB, returning the result, or `None` if no process is running.
pub fn with_current_process<R, F: FnOnce(&crate::proc::PCB) -> R>(f: F) -> Option<R> {
    proc::with_process(get_current_pid(), f)
}

/// Calls `f` with a mutable reference to the current process's PCB, returning the result, or `None` if no process is running.
pub fn with_current_process_mut<R, F: FnOnce(&mut crate::proc::PCB) -> R>(f: F) -> Option<R> {
    proc::with_process_mut(get_current_pid(), f)
}

/// Called from the timer interrupt with the interrupted thread's RSP; returns the RSP to restore.
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
        unsafe { (*old_tcb_ptr).state = ThreadState::Running; }
        return old_rsp;
    }

    let mut next_index = (old_index + 1) % count;
    let mut iterations = 0;

    loop {
        let next_tcb_ptr = get_thread(next_index);
        let (state, rsp, pid) = unsafe {
            ((*next_tcb_ptr).state, (*next_tcb_ptr).cpu_context.rsp, (*next_tcb_ptr).pid)
        };

        // Skip threads whose owning process is no longer Running.
        let process_alive = proc::with_process(pid, |p| p.state == ProcessState::Running)
            .unwrap_or(false);

        if state == ThreadState::Ready && rsp != 0 && process_alive {
            break;
        }

        next_index = (next_index + 1) % count;
        iterations += 1;

        if iterations >= count {
            unsafe { (*old_tcb_ptr).state = ThreadState::Running; }
            return old_rsp;
        }
    }

    let new_tcb_ptr = get_thread(next_index);
    unsafe { (*new_tcb_ptr).state = ThreadState::Running; }

    CURRENT_INDEX.store(next_index, Ordering::Release);
    CURRENT_THREAD.store(new_tcb_ptr, Ordering::Release);

    unsafe {
        if (*new_tcb_ptr).is_userspace && (*new_tcb_ptr).kernel_stack_top != 0 {
            write_tss_rsp0((*new_tcb_ptr).kernel_stack_top);
        }

        let new_cr3 = (*new_tcb_ptr).cr3;
        if new_cr3 != 0 {
            let current_cr3: u64;
            core::arch::asm!("mov {}, cr3", out(reg) current_cr3);
            if current_cr3 != new_cr3 {
                core::arch::asm!("mov cr3, {}", in(reg) new_cr3,
                    options(nostack, preserves_flags));
            }
        }

        let new_rsp = (*new_tcb_ptr).cpu_context.rsp;
        assert!(new_rsp != 0, "Scheduler: thread {} has null RSP", (*new_tcb_ptr).tid);
        new_rsp
    }
}

/// Returns a raw pointer to the currently running TCB.
pub fn get_current_thread() -> *mut TCB {
    CURRENT_THREAD.load(Ordering::Acquire)
}

/// Returns the TID of the currently running thread, or 0 if none is set.
pub fn get_current_tid() -> u64 {
    let ptr = get_current_thread();
    if ptr.is_null() {
        0
    } else {
        unsafe { (*ptr).tid }
    }
}

/// Installs `tcb` as the current thread pointer.
pub fn set_current_thread(tcb: *mut TCB) {
    CURRENT_THREAD.store(tcb, Ordering::Release);
}

/// Sets the scheduler's current index, used during boot to register the kernel thread.
pub fn set_current_index(index: usize) {
    CURRENT_INDEX.store(index, Ordering::Release);
}

/// Returns the scheduler's current thread index.
pub fn get_current_index() -> usize {
    CURRENT_INDEX.load(Ordering::Acquire)
}

/// Voluntarily yields the current time slice, triggering an immediate context switch if another thread is ready.
pub fn yield_now() {
    unsafe {
        let current = get_current_thread();
        if current.is_null() {
            return;
        }

        let ticks = TICK_COUNT.load(Ordering::Acquire);
        let remainder = ticks % TICKS_PER_SLICE;
        if remainder != 0 {
            TICK_COUNT.fetch_add(TICKS_PER_SLICE - remainder, Ordering::AcqRel);
        }

        let new_rsp = schedule((*current).cpu_context.rsp);

        if new_rsp == (*current).cpu_context.rsp {
            return;
        }

        unsafe extern "C" {
            unsafe fn context_switch(old_rsp: *mut u64, new_rsp: u64);
        }
        context_switch(&mut (*current).cpu_context.rsp, new_rsp);
    }
}