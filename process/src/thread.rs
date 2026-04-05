use core::{alloc::Layout, sync::atomic::{AtomicU64, AtomicUsize, Ordering}};
use alloc::alloc::alloc_zeroed;
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
    pub stack_base: *mut u8,
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
        "sti",
        "call rbx",
        "ud2",
    );
}

fn setup_stack(stack_base: *mut u8, stack_size: u64, entry: u64) -> u64 {
    unsafe {
        let top = stack_base.add(stack_size as usize) as *mut u64;

        let wrapper_addr = thread_entry_wrapper as *const () as u64;
        assert!(wrapper_addr != 0);
        assert!(entry != 0);

        // iretq frame
        *top.offset(-1) = 0x10;                        // ss
        *top.offset(-2) = top.offset(-5) as u64; // rsp
        *top.offset(-3) = 0x202;                       // rflags (IF=1)
        *top.offset(-4) = 0x08;                        // cs
        *top.offset(-5) = wrapper_addr;                // rip

        *top.offset(-6)  = 0;      // rax
        *top.offset(-7)  = entry;  // rbx
        *top.offset(-8)  = 0;      // rcx
        *top.offset(-9)  = 0;      // rdx
        *top.offset(-10) = 0;      // rsi
        *top.offset(-11) = 0;      // rdi
        *top.offset(-12) = 0;      // rbp
        *top.offset(-13) = 0;      // r8
        *top.offset(-14) = 0;      // r9
        *top.offset(-15) = 0;      // r10
        *top.offset(-16) = 0;      // r11
        *top.offset(-17) = 0;      // r12
        *top.offset(-18) = 0;      // r13
        *top.offset(-19) = 0;      // r14
        *top.offset(-20) = 0;      // r15

        top.offset(-20) as u64
    }
}

impl TCB {
    pub fn new(stack_size: u64, entry: u64) -> *mut TCB {
        let layout = Layout::from_size_align(stack_size as usize, 4096).unwrap();
        let stack_base = unsafe { alloc_zeroed(layout) };
        assert!(!stack_base.is_null());

        let rsp = setup_stack(stack_base, stack_size, entry);
        let kernel_cr3 = memory::vmm::VMM::get_page_table() as u64;
        let index = THREAD_COUNT.fetch_add(1, Ordering::AcqRel);
        assert!(index < MAX_THREADS);

        let tcb = TCB {
            tid: NEXT_THREAD_ID.fetch_add(1, Ordering::Relaxed),
            stack_size,
            stack_base,
            stack_top: unsafe { stack_base.add(stack_size as usize) },
            cpu_context: CpuContext {
                rsp,
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

    pub fn from_current_stack(tid: u64, cr3: u64, rsp: u64) -> *mut TCB {
        let index = THREAD_COUNT.fetch_add(1, Ordering::AcqRel);
        assert!(index < MAX_THREADS);

        let tcb = TCB {
            tid,
            stack_size: 0,
            stack_base: core::ptr::null_mut(),
            stack_top: core::ptr::null_mut(),
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

pub fn init_kernel_thread() {
    let kernel_pml4 = memory::vmm::VMM::get_page_table() as u64;
    let current_rsp: u64;

    unsafe {
        core::arch::asm!("mov {}, rsp", out(reg) current_rsp);
    }

    let tcb = TCB::from_current_stack(0, kernel_pml4, current_rsp);
    scheduler::set_current_thread(tcb);
    scheduler::set_current_index(0);
    println!("Kernel process initialized at RSP: {:#x}", current_rsp);
}