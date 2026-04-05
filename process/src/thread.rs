use core::{alloc::Layout, sync::atomic::{AtomicU64, AtomicUsize, Ordering}};
use alloc::alloc::alloc;
use framebuffer::println;
use crate::scheduler;

pub type ThreadId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadState {
    Ready,
    Running,
    Blocked,
    Sleeping,
    Zombie,
    Dead,
}

#[derive(Debug, Default, Clone)]
#[repr(C)]
pub struct CpuContext {
    pub rbx: u64,
    pub rbp: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub rip: u64,
    pub rsp: u64,
    pub rflags: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Priority(pub u8);

impl Priority {
    pub const IDLE: Self     = Self(0);
    pub const LOW: Self      = Self(64);
    pub const NORMAL: Self   = Self(128);
    pub const HIGH: Self     = Self(192);
    pub const REALTIME: Self = Self(255);
}

#[derive(Debug)]
pub struct TCB {
    pub tid: ThreadId,
    pub stack_size: u64,
    pub stack_top: *mut u8,
    pub cpu_context: CpuContext,
    pub next: *mut TCB,
    pub cr3: u64,
    pub state: ThreadState,
    pub priority: Priority,
}

const MAX_THREADS: usize = 16;
static mut THREAD_STORAGE: [core::mem::MaybeUninit<TCB>; MAX_THREADS] =
    [const { core::mem::MaybeUninit::uninit() }; MAX_THREADS];
static THREAD_COUNT: AtomicUsize = AtomicUsize::new(0);
static NEXT_THREAD_ID: AtomicU64 = AtomicU64::new(1);

pub fn get_thread_count() -> usize {
    THREAD_COUNT.load(Ordering::Acquire)
}

pub fn get_thread(index: usize) -> *mut TCB {
    unsafe { THREAD_STORAGE[index].as_mut_ptr() }
}

#[unsafe(naked)]
extern "C" fn thread_entry_wrapper() {
    core::arch::naked_asm!(
        "call rbx",
        "ud2",
    );
}

impl TCB {
    pub fn new(stack_size: u64, entry: u64) -> *mut TCB {
        let layout = Layout::from_size_align(stack_size as usize, 16).unwrap();
        let stack_bottom = unsafe { alloc(layout) };
        let stack_top = unsafe { stack_bottom.add(stack_size as usize) };

        let mut sp = stack_top as *mut u64;

        unsafe {
            sp = sp.sub(1); *sp = 0x10u64;
            sp = sp.sub(1); *sp = stack_top as u64;
            sp = sp.sub(1); *sp = 0x202u64;
            sp = sp.sub(1); *sp = 0x08u64;
            sp = sp.sub(1); *sp = thread_entry_wrapper as *const () as u64;

            sp = sp.sub(1); *sp = 0u64; // rax
            sp = sp.sub(1); *sp = entry; // rbx
            sp = sp.sub(1); *sp = 0u64; // rcx
            sp = sp.sub(1); *sp = 0u64; // rdx
            sp = sp.sub(1); *sp = 0u64; // rsi
            sp = sp.sub(1); *sp = 0u64; // rdi
            sp = sp.sub(1); *sp = 0u64; // rbp
            sp = sp.sub(1); *sp = 0u64; // r8
            sp = sp.sub(1); *sp = 0u64; // r9
            sp = sp.sub(1); *sp = 0u64; // r10
            sp = sp.sub(1); *sp = 0u64; // r11
            sp = sp.sub(1); *sp = 0u64; // r12
            sp = sp.sub(1); *sp = 0u64; // r13
            sp = sp.sub(1); *sp = 0u64; // r14
            sp = sp.sub(1); *sp = 0u64; // r15
        }

        let kernel_cr3 = memory::vmm::VMM::get_page_table() as u64;
        let index = THREAD_COUNT.fetch_add(1, Ordering::AcqRel);
        assert!(index < MAX_THREADS, "Too many threads");

        let tcb = TCB {
            tid: NEXT_THREAD_ID.fetch_add(1, Ordering::Relaxed),
            stack_size,
            stack_top,
            cpu_context: CpuContext {
                rsp: sp as u64,
                ..CpuContext::default()
            },
            next: core::ptr::null_mut(),
            cr3: kernel_cr3,
            state: ThreadState::Ready,
            priority: Priority::NORMAL,
        };

        unsafe {
            THREAD_STORAGE[index].write(tcb);
            THREAD_STORAGE[index].as_mut_ptr()
        }
    }

    pub fn from_existing(tid: u64, rsp: u64, cr3: u64) -> *mut TCB {
        let index = THREAD_COUNT.fetch_add(1, Ordering::AcqRel);
        assert!(index < MAX_THREADS, "Too many threads");

        let tcb = TCB {
            tid,
            stack_size: 0,
            stack_top: rsp as *mut u8,
            cpu_context: CpuContext {
                rsp,
                ..CpuContext::default()
            },
            next: core::ptr::null_mut(),
            cr3,
            state: ThreadState::Running,
            priority: Priority::HIGH,
        };

        unsafe {
            THREAD_STORAGE[index].write(tcb);
            THREAD_STORAGE[index].as_mut_ptr()
        }
    }
}

pub fn init_kernel_process(rsp: u64) {
    let kernel_pml4 = memory::vmm::VMM::get_page_table() as u64;
    let tcb = TCB::from_existing(0, rsp, kernel_pml4);
    scheduler::set_current_thread(tcb);
    scheduler::set_current_index(0);
    println!("Kernel process initialized at RSP: 0x{:X}", rsp);
}