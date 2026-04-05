//! APIC support for x86_64 architecture
//!
//! The APIC is the modern replacement for the obsolete PIT timer,
//! with better multi-core support and additional features.
//!
use super::cpu_types::CPUFeatures;
use super::msr::{read_msr, write_msr};
use core::sync::atomic::{AtomicUsize, Ordering};

const APIC_BASE_MSR: u32 = 0x1B;
const APIC_BASE_MSR_ENABLE: u64 = 0x800;
const APIC_SPURIOUS_INTERRUPT_VECTOR: usize = 0xF0;
const APIC_SOFTWARE_ENABLE: u32 = 0x100;
const APIC_TIMER_LVT: usize = 0x320;
const APIC_TIMER_INITIAL_COUNT: usize = 0x380;
const APIC_TIMER_CURRENT_COUNT: usize = 0x390;
const APIC_TIMER_DIVIDE_CONFIG: usize = 0x3E0;
const APIC_EOI: usize = 0xB0;

const IOAPIC_IOREGSEL: usize = 0x00;
const IOAPIC_IOWIN: usize = 0x10;
const IOAPIC_ID: u32 = 0x00;
const IOAPIC_VER: u32 = 0x01;
const IOAPIC_REDTBL_BASE: u32 = 0x10;

const IOAPIC_MASKED: u64 = 1 << 16;
const IOAPIC_LEVEL_TRIGGERED: u64 = 1 << 15;
const IOAPIC_ACTIVE_LOW: u64 = 1 << 13;

static APIC_VIRT_BASE: AtomicUsize = AtomicUsize::new(0);
static IOAPIC_VIRT_BASE: AtomicUsize = AtomicUsize::new(0);

#[unsafe(no_mangle)]
pub static mut APIC_TICKS_PER_SEC: u64 = 0;

fn set_apic_base(apic: usize) {
    let eax: u32 = ((apic & 0xfffff000) | APIC_BASE_MSR_ENABLE as usize) as u32;
    let edx: u32 = 0;
    write_msr(APIC_BASE_MSR, ((edx as u64) << 32) | (eax as u64));
}

pub fn get_apic_base() -> usize {
    let msr_value: u64 = read_msr(APIC_BASE_MSR);
    (msr_value as usize) & 0xfffff000
}

pub fn set_apic_virt_base(virt_addr: usize) {
    APIC_VIRT_BASE.store(virt_addr, Ordering::SeqCst);
}

/// Set the virtual address where the IOAPIC is mapped
pub fn set_ioapic_virt_base(virt_addr: usize) {
    IOAPIC_VIRT_BASE.store(virt_addr, Ordering::SeqCst);
}

fn read_apic_register(offset: usize) -> u32 {
    let apic_base = APIC_VIRT_BASE.load(Ordering::SeqCst);
    let register = (apic_base + offset) as *const u32;
    unsafe { core::ptr::read_volatile(register) }
}

fn write_apic_register(offset: usize, value: u32) {
    let apic_base = APIC_VIRT_BASE.load(Ordering::SeqCst);
    let register = (apic_base + offset) as *mut u32;
    unsafe { core::ptr::write_volatile(register, value) };
}

fn read_ioapic_register(reg: u32) -> u32 {
    let base = IOAPIC_VIRT_BASE.load(Ordering::SeqCst);
    unsafe {
        core::ptr::write_volatile((base + IOAPIC_IOREGSEL) as *mut u32, reg);
        core::ptr::read_volatile((base + IOAPIC_IOWIN) as *const u32)
    }
}

fn write_ioapic_register(reg: u32, value: u32) {
    let base = IOAPIC_VIRT_BASE.load(Ordering::SeqCst);
    unsafe {
        core::ptr::write_volatile((base + IOAPIC_IOREGSEL) as *mut u32, reg);
        core::ptr::write_volatile((base + IOAPIC_IOWIN) as *mut u32, value);
    }
}

/// Read a 64-bit redirection table entry
fn read_ioapic_redtbl(irq: u8) -> u64 {
    let lo = read_ioapic_register(IOAPIC_REDTBL_BASE + (irq as u32) * 2) as u64;
    let hi = read_ioapic_register(IOAPIC_REDTBL_BASE + (irq as u32) * 2 + 1) as u64;
    lo | (hi << 32)
}

fn write_ioapic_redtbl(irq: u8, entry: u64) {
    write_ioapic_register(IOAPIC_REDTBL_BASE + (irq as u32) * 2, entry as u32);
    write_ioapic_register(IOAPIC_REDTBL_BASE + (irq as u32) * 2 + 1, (entry >> 32) as u32);
}

pub fn enable_apic() {
    let cpu_features = CPUFeatures::detect();
    if !cpu_features.apic {
        panic!("APIC not supported on this CPU");
    }
    set_apic_base(get_apic_base());
    let svr = read_apic_register(APIC_SPURIOUS_INTERRUPT_VECTOR);
    write_apic_register(APIC_SPURIOUS_INTERRUPT_VECTOR, svr | APIC_SOFTWARE_ENABLE);
}

pub fn init_ioapic() {
    let max_redir = (read_ioapic_register(IOAPIC_VER) >> 16) & 0xFF;
    for irq in 0..=max_redir as u8 {
        let entry = read_ioapic_redtbl(irq) | IOAPIC_MASKED;
        write_ioapic_redtbl(irq, entry);
    }
}

/// Configure and unmask an IOAPIC IRQ line.
///
/// - `irq`: hardware IRQ number (0–23 typically)
/// - `vector`: IDT vector to deliver to (32–255)
/// - `dest_apic_id`: local APIC ID of the target CPU
/// - `level_triggered`: true = level, false = edge
/// - `active_low`: true = active low, false = active high
pub fn ioapic_set_irq(irq: u8, vector: u8, dest_apic_id: u8, level_triggered: bool, active_low: bool) {
    let mut entry = vector as u64;
    if level_triggered { entry |= IOAPIC_LEVEL_TRIGGERED; }
    if active_low      { entry |= IOAPIC_ACTIVE_LOW; }
    // physical destination mode, delivery mode = fixed (000)
    entry |= (dest_apic_id as u64) << 56;
    write_ioapic_redtbl(irq, entry);
}

/// Mask a single IOAPIC IRQ line
pub fn ioapic_mask_irq(irq: u8) {
    let entry = read_ioapic_redtbl(irq) | IOAPIC_MASKED;
    write_ioapic_redtbl(irq, entry);
}

/// Unmask a single IOAPIC IRQ line
pub fn ioapic_unmask_irq(irq: u8) {
    let entry = read_ioapic_redtbl(irq) & !IOAPIC_MASKED;
    write_ioapic_redtbl(irq, entry);
}

/// Get the IOAPIC ID
pub fn ioapic_id() -> u8 {
    ((read_ioapic_register(IOAPIC_ID) >> 24) & 0xF) as u8
}

/// Get the IOAPIC version and max redirection entries
pub fn ioapic_version() -> (u8, u8) {
    let ver = read_ioapic_register(IOAPIC_VER);
    let version = (ver & 0xFF) as u8;
    let max_redir = ((ver >> 16) & 0xFF) as u8;
    (version, max_redir)
}

/// Initialize the APIC timer
pub fn init_apic_timer(interrupt_vector: u8, initial_count: u32) {
    write_apic_register(APIC_TIMER_DIVIDE_CONFIG, 0x3);
    write_apic_register(APIC_TIMER_LVT, (interrupt_vector as u32) | (1 << 17));
    write_apic_register(APIC_TIMER_INITIAL_COUNT, initial_count);
}

/// Send End-Of-Interrupt signal to the Local APIC
pub fn apic_eoi() {
    write_apic_register(APIC_EOI, 0);
}

/// Calibrate it using tsc
pub fn calibrate_apic_timer(target_hz: u64) -> u32 {
    const PIT_FREQUENCY: u64 = 1_193_182;
    const CALIBRATION_MS: u64 = 10;
    const PIT_TICKS: u16 = (PIT_FREQUENCY * CALIBRATION_MS / 1000) as u16;

    unsafe {
        // Set PIT channel 2 for one-shot countdown
        core::arch::asm!(
            // Gate on, speaker off
            "in al, 0x61",
            "and al, 0xFC",
            "or al, 0x01",
            "out 0x61, al",
            // Channel 2, lobyte/hibyte, mode 0 (interrupt on terminal count)
            "mov al, 0xB0",
            "out 0x43, al",
            // Load count
            "mov al, {lo}",
            "out 0x42, al",
            "mov al, {hi}",
            "out 0x42, al",
            lo = in(reg_byte) (PIT_TICKS & 0xFF) as u8,
            hi = in(reg_byte) (PIT_TICKS >> 8) as u8,
            out("al") _,
        );
    }

    write_apic_register(APIC_TIMER_DIVIDE_CONFIG, 0x3);
    write_apic_register(APIC_TIMER_LVT, 1 << 16);
    write_apic_register(APIC_TIMER_INITIAL_COUNT, u32::MAX);

    // Wait for PIT channel 2 output to go high (bit 5 of port 0x61)
    loop {
        let val: u8;
        unsafe {
            core::arch::asm!(
                "in al, 0x61",
                out("al") val,
                options(nomem, nostack),
            );
        }
        if val & 0x20 != 0 {
            break;
        }
    }

    let elapsed = u32::MAX - read_apic_register(APIC_TIMER_CURRENT_COUNT);
    let apic_freq = (elapsed as u64) * (1000 / CALIBRATION_MS);
    let initial = apic_freq / target_hz;

    unsafe {
        APIC_TICKS_PER_SEC = target_hz;
    }

    initial as u32
}