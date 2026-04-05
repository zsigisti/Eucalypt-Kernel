pub const HHDM_OFFSET: usize = 0xFFFF800000000000;

pub fn phys_to_virt(phys_addr: usize) -> usize {
    phys_addr + HHDM_OFFSET
}

pub fn virt_to_phys(virt_addr: usize) -> usize {
    virt_addr - HHDM_OFFSET
}