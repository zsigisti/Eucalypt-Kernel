use core::{alloc::Layout, sync::atomic::{AtomicU64, AtomicUsize, Ordering}};
use alloc::alloc::alloc_zeroed;
use framebuffer::println;
use crate::scheduler;
use memory::paging::PageTableEntry;
use memory::addr::VirtAddr;
use memory::frame_allocator::FrameAllocator;
use memory::vmm::VMM;

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
    pub is_userspace: bool,
    pub kernel_stack_top: u64,
}

const MAX_THREADS: usize = 16;
static mut THREAD_STORAGE: [core::mem::MaybeUninit<TCB>; MAX_THREADS] =
    [const { core::mem::MaybeUninit::uninit() }; MAX_THREADS];
static THREAD_COUNT: AtomicUsize = AtomicUsize::new(0);
static NEXT_THREAD_ID: AtomicU64 = AtomicU64::new(1);

const USER_STACK_SIZE: u64   = 0x8000;
const USER_STACK_TOP:  u64   = 0x0000_7FFF_FFFF_0000;
const HHDM_OFFSET:     u64   = 0xFFFF800000000000;
const PAGE_SIZE:       usize = 0x1000;
const USER_CS:         u64   = 0x1B;
const USER_SS:         u64   = 0x23;

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

#[unsafe(naked)]
#[unsafe(no_mangle)]
unsafe extern "C" fn iretq_trampoline() {
    core::arch::naked_asm!(
        "iretq",
    );
}

fn setup_stack(stack_base: *mut u8, stack_size: u64, entry: u64) -> u64 {
    unsafe {
        let top = stack_base.add(stack_size as usize) as *mut u64;
        let wrapper_addr = thread_entry_wrapper as *const () as u64;
        assert!(wrapper_addr != 0);
        assert!(entry != 0);
        *top.offset(-1)  = 0x10;
        *top.offset(-2)  = top.offset(-5) as u64;
        *top.offset(-3)  = 0x202;
        *top.offset(-4)  = 0x08;
        *top.offset(-5)  = wrapper_addr;
        *top.offset(-6)  = 0;
        *top.offset(-7)  = entry;
        *top.offset(-8)  = 0;
        *top.offset(-9)  = 0;
        *top.offset(-10) = 0;
        *top.offset(-11) = 0;
        *top.offset(-12) = 0;
        *top.offset(-13) = 0;
        *top.offset(-14) = 0;
        *top.offset(-15) = 0;
        *top.offset(-16) = 0;
        *top.offset(-17) = 0;
        *top.offset(-18) = 0;
        *top.offset(-19) = 0;
        *top.offset(-20) = 0;
        top.offset(-20) as u64
    }
}

fn setup_user_stack(stack_base: *mut u8, stack_size: u64, entry: u64) -> u64 {
    unsafe {
        let top = stack_base.add(stack_size as usize) as *mut u64;

        // iretq frame
        *top.offset(-1) = USER_SS;
        *top.offset(-2) = USER_STACK_TOP;
        *top.offset(-3) = 0x202;
        *top.offset(-4) = USER_CS;
        *top.offset(-5) = entry;

        // 15 general purpose registers (popped by timer handler)
        *top.offset(-6)  = 0; // r15
        *top.offset(-7)  = 0; // r14
        *top.offset(-8)  = 0; // r13
        *top.offset(-9)  = 0; // r12
        *top.offset(-10) = 0; // r11
        *top.offset(-11) = 0; // r10
        *top.offset(-12) = 0; // r9
        *top.offset(-13) = 0; // r8
        *top.offset(-14) = 0; // rbp
        *top.offset(-15) = 0; // rdi
        *top.offset(-16) = 0; // rsi
        *top.offset(-17) = 0; // rdx
        *top.offset(-18) = 0; // rcx
        *top.offset(-19) = 0; // rbx
        *top.offset(-20) = 0; // rax

        top.offset(-20) as u64
    }
}

fn alloc_user_stack(pml4: *mut memory::paging::PageTable) -> Option<*mut u8> {
    let mut mapper = VMM::get_mapper();
    let pages = USER_STACK_SIZE as usize / PAGE_SIZE;
    let flags = PageTableEntry::PRESENT
              | PageTableEntry::WRITABLE
              | PageTableEntry::USER
              | PageTableEntry::NO_EXECUTE;

    for i in 0..pages {
        let frame = FrameAllocator::alloc_frame()?;
        let virt  = VirtAddr::new(USER_STACK_TOP - USER_STACK_SIZE + (i * PAGE_SIZE) as u64);
        mapper.map_page(pml4, virt, frame, flags)?;
    }

    Some((USER_STACK_TOP - USER_STACK_SIZE) as *mut u8)
}

impl TCB {
    pub fn new(stack_size: u64, entry: u64) -> *mut TCB {
        let layout = Layout::from_size_align(stack_size as usize, 4096).unwrap();
        let stack_base = unsafe { alloc_zeroed(layout) };
        assert!(!stack_base.is_null());

        let rsp = setup_stack(stack_base, stack_size, entry);
        let kernel_cr3 = VMM::get_page_table() as u64;
        let index = THREAD_COUNT.fetch_add(1, Ordering::AcqRel);
        assert!(index < MAX_THREADS);

        let stack_top = unsafe { stack_base.add(stack_size as usize) };

        let tcb = TCB {
            tid: NEXT_THREAD_ID.fetch_add(1, Ordering::Relaxed),
            stack_size,
            stack_base,
            stack_top,
            cpu_context: CpuContext {
                rsp,
                ..CpuContext::default()
            },
            next: core::ptr::null_mut(),
            cr3: kernel_cr3,
            state: ThreadState::Ready,
            priority: Priority::NORMAL,
            is_userspace: false,
            kernel_stack_top: stack_top as u64,
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
            is_userspace: false,
            kernel_stack_top: 0,
        };

        unsafe {
            THREAD_STORAGE[index].write(tcb);
            THREAD_STORAGE[index].as_mut_ptr()
        }
    }
}

pub fn spawn_userspace_process(entry: u64, pml4_phys: u64) -> *mut TCB {
    let kstack_size = 0x4000u64;
    let layout = Layout::from_size_align(kstack_size as usize, 4096).unwrap();
    let kstack_base = unsafe { alloc_zeroed(layout) };
    assert!(!kstack_base.is_null());

    let kstack_top = unsafe { kstack_base.add(kstack_size as usize) };

    let pml4_ptr = (pml4_phys | HHDM_OFFSET) as *mut memory::paging::PageTable;
    alloc_user_stack(pml4_ptr).expect("spawn_userspace_process: user stack mapping failed");

    let rsp = setup_user_stack(kstack_base, kstack_size, entry);
    let index = THREAD_COUNT.fetch_add(1, Ordering::AcqRel);
    assert!(index < MAX_THREADS);

    let tcb = TCB {
        tid: NEXT_THREAD_ID.fetch_add(1, Ordering::Relaxed),
        stack_size: kstack_size,
        stack_base: kstack_base,
        stack_top: kstack_top,
        cpu_context: CpuContext {
            rsp,
            ..CpuContext::default()
        },
        next: core::ptr::null_mut(),
        cr3: pml4_phys,
        state: ThreadState::Ready,
        priority: Priority::NORMAL,
        is_userspace: true,
        kernel_stack_top: kstack_top as u64,
    };

    unsafe {
        THREAD_STORAGE[index].write(tcb);
        THREAD_STORAGE[index].as_mut_ptr()
    }
}

pub fn init_kernel_thread() {
    let kernel_pml4 = VMM::get_page_table() as u64;
    let current_rsp: u64;

    unsafe {
        core::arch::asm!("mov {}, rsp", out(reg) current_rsp);
    }

    let tcb = TCB::from_current_stack(0, kernel_pml4, current_rsp);
    scheduler::set_current_thread(tcb);
    scheduler::set_current_index(0);
    println!("Kernel process initialized at RSP: {:#x}", current_rsp);
}