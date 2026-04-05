use core::ptr::addr_of_mut;
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use bare_x86_64::cpu::apic;
use ide::{ide_primary_irq_handler, ide_secondary_irq_handler};
use x86_64::registers::control::Cr2;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};

const APIC_TIMER_VECTOR: u8 = 32;
const IDE_PRIMARY_VECTOR: u8 = 33;
const IDE_SECONDARY_VECTOR: u8 = 34;

const IDE_PRIMARY_IRQ: u8 = 14;
const IDE_SECONDARY_IRQ: u8 = 15;

static mut IDT: InterruptDescriptorTable = InterruptDescriptorTable::new();
static TIMER_TICKS: AtomicU64 = AtomicU64::new(0);

macro_rules! register_exceptions {
    ($idt:expr, $(
        $field:ident : $name:ident, $msg:literal $(, $kind:ident)?;
    )*) => {
        $(
            register_exceptions!(@handler $name, $msg $(, $kind)?);
            $idt.$field.set_handler_fn($name);
        )*
    };

    (@handler $name:ident, $msg:literal) => {
        extern "x86-interrupt" fn $name(sf: InterruptStackFrame) {
            panic!("EXCEPTION: {}\n{:#?}", $msg, sf);
        }
    };
    (@handler $name:ident, $msg:literal, error) => {
        extern "x86-interrupt" fn $name(sf: InterruptStackFrame, ec: u64) {
            panic!("EXCEPTION: {}\nError Code: {}\n{:#?}", $msg, ec, sf);
        }
    };
    (@handler $name:ident, $msg:literal, diverging) => {
        extern "x86-interrupt" fn $name(sf: InterruptStackFrame, ec: u64) -> ! {
            panic!("EXCEPTION: {}\nError Code: {}\n{:#?}", $msg, ec, sf);
        }
    };
    (@handler $name:ident, $msg:literal, diverging_no_error) => {
        extern "x86-interrupt" fn $name(sf: InterruptStackFrame) -> ! {
            panic!("EXCEPTION: {}\n{:#?}", $msg, sf);
        }
    };
}

pub fn idt_init() {
    let idt: &mut InterruptDescriptorTable = unsafe { &mut *addr_of_mut!(IDT) };

    register_exceptions!(idt,
        divide_error             : divide_error_handler,            "DIVIDE ERROR";
        debug                    : debug_handler,                   "DEBUG";
        non_maskable_interrupt   : nmi_handler,                     "NON-MASKABLE INTERRUPT";
        breakpoint               : breakpoint_handler,              "BREAKPOINT";
        overflow                 : overflow_handler,                "OVERFLOW";
        bound_range_exceeded     : bound_range_handler,             "BOUND RANGE EXCEEDED";
        invalid_opcode           : invalid_opcode_handler,          "INVALID OPCODE";
        device_not_available     : device_not_available_handler,    "DEVICE NOT AVAILABLE";
        double_fault             : double_fault_handler,            "DOUBLE FAULT",             diverging;
        invalid_tss              : invalid_tss_handler,             "INVALID TSS",              error;
        segment_not_present      : segment_not_present_handler,     "SEGMENT NOT PRESENT",      error;
        stack_segment_fault      : stack_segment_fault_handler,     "STACK SEGMENT FAULT",      error;
        general_protection_fault : gpf_handler,                     "GENERAL PROTECTION FAULT", error;
        x87_floating_point       : x87_handler,                     "x87 FLOATING POINT";
        alignment_check          : alignment_check_handler,         "ALIGNMENT CHECK",          error;
        machine_check            : machine_check_handler,           "MACHINE CHECK",            diverging_no_error;
        simd_floating_point      : simd_handler,                    "SIMD FLOATING POINT";
        virtualization           : virtualization_handler,          "VIRTUALIZATION";
        security_exception       : security_exception_handler,      "SECURITY EXCEPTION",       error;
    );

    idt.page_fault.set_handler_fn(page_fault_handler);

    set_raw_idt_entry(idt, APIC_TIMER_VECTOR, apic_timer_handler as *const () as u64);

    idt[IDE_PRIMARY_VECTOR].set_handler_fn(ide_primary_handler);
    idt[IDE_SECONDARY_VECTOR].set_handler_fn(ide_secondary_handler);

    idt.load();

    apic::ioapic_set_irq(IDE_PRIMARY_IRQ,   IDE_PRIMARY_VECTOR,   0, false, false);
    apic::ioapic_set_irq(IDE_SECONDARY_IRQ, IDE_SECONDARY_VECTOR, 0, false, false);
}

fn set_raw_idt_entry(idt: &mut InterruptDescriptorTable, index: u8, handler_addr: u64) {
    let idt_ptr = idt as *mut InterruptDescriptorTable as *mut u64;
    let entry_ptr = unsafe { idt_ptr.add(index as usize * 2) };
    let low = (handler_addr & 0xFFFF)
        | (0x08 << 16)
        | (0x8E00 << 32)
        | ((handler_addr & 0xFFFF_0000) << 32);
    let high = handler_addr >> 32;
    unsafe {
        *entry_ptr = low;
        *entry_ptr.add(1) = high;
    }
}

#[unsafe(naked)]
extern "C" fn apic_timer_handler() {
    unsafe {
        core::arch::naked_asm!(
            "push rax", "push rbx", "push rcx", "push rdx",
            "push rsi", "push rdi", "push rbp", "push r8",
            "push r9", "push r10", "push r11", "push r12",
            "push r13", "push r14", "push r15",
            "mov rdi, rsp",
            "call {handler}",
            "mov rsp, rax",
            "pop r15", "pop r14", "pop r13", "pop r12",
            "pop r11", "pop r10", "pop r9", "pop r8",
            "pop rbp", "pop rdi", "pop rsi", "pop rdx",
            "pop rcx", "pop rbx", "pop rax",
            "iretq",
            handler = sym apic_timer_interrupt_handler,
        );
    }
}

static DBG_COUNT: AtomicUsize = AtomicUsize::new(0);
static DBG_OLD: AtomicUsize = AtomicUsize::new(0);
static DBG_NEW: AtomicUsize = AtomicUsize::new(0);

#[unsafe(no_mangle)]
extern "C" fn apic_timer_interrupt_handler(rsp: u64) -> u64 {
    TIMER_TICKS.fetch_add(1, Ordering::Relaxed);
    apic::apic_eoi();

    DBG_COUNT.store(process::thread::get_thread_count(), Ordering::Relaxed);
    DBG_OLD.store(process::scheduler::get_current_index(), Ordering::Relaxed);
    let new_rsp = process::scheduler::schedule(rsp);
    DBG_NEW.store(process::scheduler::get_current_index(), Ordering::Relaxed);

    new_rsp
}

pub fn get_dbg() -> (usize, usize, usize) {
    (
        DBG_COUNT.load(Ordering::Relaxed),
        DBG_OLD.load(Ordering::Relaxed),
        DBG_NEW.load(Ordering::Relaxed),
    )
}

pub fn get_timer_ticks() -> u64 {
    TIMER_TICKS.load(Ordering::Relaxed)
}

extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    panic!(
        "EXCEPTION: PAGE FAULT\nAccessed Address: {:?}\nError Code: {:?}\n{:#?}",
        Cr2::read(),
        error_code,
        stack_frame
    );
}

extern "x86-interrupt" fn ide_primary_handler(_stack_frame: InterruptStackFrame) {
    ide_primary_irq_handler();
    apic::apic_eoi();
}

extern "x86-interrupt" fn ide_secondary_handler(_stack_frame: InterruptStackFrame) {
    ide_secondary_irq_handler();
    apic::apic_eoi();
}