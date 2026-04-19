#![no_std]
use memory::{addr::{PhysAddr, VirtAddr}, vmm::VMM};
use pci::pci_find_nvme_controller;

const CAP_OFF: usize = 0x00;
const VS_OFF: usize = 0x08;
const CC_OFF: usize = 0x14;
const CSTS_OFF: usize = 0x1C;
const AQA_OFF: usize = 0x24;
const ASQ_OFF: usize = 0x28;
const ACQ_OFF: usize = 0x30;

struct NvmeQueue {
    sq: *mut u8,
    cq: *mut u8,
    sq_tail: usize,
    cq_head: usize,
    qid: usize,
}

static mut NVME_BASE: u64 = 0;
static mut NVME_CAP: u64 = 0;

fn read_reg32(offset: usize) -> u32 {
    let ptr = (unsafe { NVME_BASE } + offset as u64) as *const u32;
    unsafe { ptr.read_volatile() }
}

fn write_reg32(offset: usize, val: u32) {
    let ptr = (unsafe { NVME_BASE } + offset as u64) as *mut u32;
    unsafe { ptr.write_volatile(val) };
}

fn read_reg64(offset: usize) -> u64 {
    let ptr = (unsafe { NVME_BASE } + offset as u64) as *const u64;
    unsafe { ptr.read_volatile() }
}

fn write_reg64(offset: usize, val: u64) {
    let ptr = (unsafe { NVME_BASE } + offset as u64) as *mut u64;
    unsafe { ptr.write_volatile(val) }
}

fn sq_tail_doorbell(queue_index: usize, dstrd: usize) -> usize {
    0x1000 + (2 * queue_index) * (4 << dstrd)
}

fn cq_head_doorbell(queue_index: usize, dstrd: usize) -> usize {
    0x1000 + (2 * queue_index + 1) * (4 << dstrd)
}

fn create_sub_queue(admin_sq: *mut NvmeQueue, new_sq: *mut NvmeQueue, cap: u64) -> bool {
    let dstrd = ((cap >> 32) & 0xF) as usize;

    unsafe {
        let sq = &mut *admin_sq;
        let new = &*new_sq;

        let slot = sq.sq.add(sq.sq_tail * 64) as *mut u32;

        // DW0: Opcode 0x01 = Create I/O Submission Queue, CID = new_sq.qid
        slot.add(0).write_volatile(0x01 | ((new.qid as u32) << 16));
        // DW1: NSID (unused for admin commands)
        slot.add(1).write_volatile(0);
        // DW2-DW3: reserved
        slot.add(2).write_volatile(0);
        slot.add(3).write_volatile(0);
        // DW4-DW5: metadata pointer (unused)
        slot.add(4).write_volatile(0);
        slot.add(5).write_volatile(0);
        // DW6-DW7: PRP1 — physical address of the new SQ
        let sq_phys = new.sq as u64;
        slot.add(6).write_volatile(sq_phys as u32);
        slot.add(7).write_volatile((sq_phys >> 32) as u32);
        // DW8-DW9: PRP2 (unused for single-page queue)
        slot.add(8).write_volatile(0);
        slot.add(9).write_volatile(0);
        // DW10: Queue ID | Queue Size (0-based, so entries - 1)
        let qsize: u32 = 63; // 64 entries
        slot.add(10).write_volatile((new.qid as u32) | (qsize << 16));
        // DW11: Completion Queue ID to pair with | Physically Contiguous bit
        slot.add(11).write_volatile((new.qid as u32) << 16 | 0x1);

        sq.sq_tail = (sq.sq_tail + 1) % 64;

        // Ring the admin SQ tail doorbell (queue 0)
        let doorbell = sq_tail_doorbell(0, dstrd);
        write_reg32(doorbell, sq.sq_tail as u32);
    }

    true // Should actually poll CQ for completion status
}

pub fn nvme_init() {
    let nvme_dev = match pci_find_nvme_controller() {
        Some(d) => d,
        None => return,
    };

    let bar0 = (nvme_dev.bar[0] & 0xFFFF_FFF0) as u64;
    let bar1 = (nvme_dev.bar[1] as u64) << 32;
    let base = bar1 | bar0;

    unsafe {
        NVME_BASE = base;
    }

    let pml4 = match VMM::get_mapper().create_user_page_table() {
        Some(table) => table,
        None => return,
    };
    VMM::get_mapper().map_page(pml4, VirtAddr::new(base), PhysAddr::new(base), 0x3);

    unsafe {
        NVME_CAP = read_reg64(CAP_OFF);
    }
}