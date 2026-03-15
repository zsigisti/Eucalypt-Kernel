//! Object based syscall dispatch and handler implementations.

use limine::request::FramebufferRequest;
use framebuffer::println;
use vfs::{vfs_close, vfs_open};

unsafe extern "C" {
    static FRAMEBUFFER_REQUEST: FramebufferRequest;
}

const ENOSYS: i64 = -38;

#[repr(u64)]
pub enum Syscall {
    PlotPoint       = 0,
    FramebufferInfo = 1,
    Print           = 2,
    Open            = 3,
    Close           = 4,
}

impl Syscall {
    pub fn from_u64(n: u64) -> Option<Self> {
        match n {
            0 => Some(Self::PlotPoint),
            1 => Some(Self::FramebufferInfo),
            2 => Some(Self::Print),
            3 => Some(Self::Open),
            4 => Some(Self::Close),
            _ => None,
        }
    }
}

pub struct SyscallHandler;

impl SyscallHandler {
    pub fn new() -> Self {
        Self
    }

    pub fn handle(&self, syscall_number: u64, arg1: i64, arg2: i64, arg3: i64) -> i64 {
        match Syscall::from_u64(syscall_number) {
            Some(Syscall::PlotPoint)       => self.plot_point(arg1, arg2, arg3),
            Some(Syscall::FramebufferInfo) => self.framebuffer_info(arg1),
            Some(Syscall::Print)           => self.print(arg1, arg2),
            Some(Syscall::Open)            => self.open(arg1, arg2, arg3),
            Some(Syscall::Close)           => self.close(arg1),
            None => ENOSYS,
        }
    }

    fn get_framebuffer(&'_ self) -> Option<limine::framebuffer::Framebuffer<'_>> {
        unsafe { FRAMEBUFFER_REQUEST.get_response() }?
            .framebuffers()
            .next()
    }

    fn plot_point(&self, x: i64, y: i64, color: i64) -> i64 {
        if let Some(fb) = self.get_framebuffer() {
            if x < 0 || y < 0
                || x >= fb.width() as i64
                || y >= fb.height() as i64
            {
                return -1;
            }
            let pitch  = fb.pitch() as i64;
            let offset = (y * pitch + x * 4) as usize;
            unsafe {
                fb.addr().add(offset).cast::<u32>().write(color as u32);
            }
        }
        0
    }

    fn framebuffer_info(&self, query: i64) -> i64 {
        if let Some(fb) = self.get_framebuffer() {
            match query {
                0 => fb.width() as i64,
                1 => fb.height() as i64,
                2 => fb.pitch() as i64,
                3 => fb.bpp() as i64,
                _ => 0,
            }
        } else {
            0
        }
    }

    fn print(&self, ptr: i64, len: i64) -> i64 {
        let ptr = ptr as *const u8;
        let len = len as usize;
        if !ptr.is_null() && len > 0 {
            let slice = unsafe { core::slice::from_raw_parts(ptr, len) };
            if let Ok(s) = core::str::from_utf8(slice) {
                println!("{}", s);
            }
        }
        0
    }

    fn open(&self, ptr: i64, flags: i64, mode: i64) -> i64 {
        let ptr = ptr as *const u8;
        let len = flags as usize;
        if ptr.is_null() || len == 0 {
            return -1;
        }
        let slice = unsafe { core::slice::from_raw_parts(ptr, len) };
        let path = match core::str::from_utf8(slice) {
            Ok(s)  => s,
            Err(_) => return -1,
        };
        match vfs_open(path, flags as u32, mode as u32) {
            Ok(node) => node.id as i64,
            Err(_)   => -1,
        }
    }

    fn close(&self, node_id: i64) -> i64 {
        match vfs_close(node_id as u32) {
            Ok(_)  => 0,
            Err(_) => -1,
        }
    }
}

pub extern "C" fn syscall_handler(syscall_number: u64, arg1: i64, arg2: i64, arg3: i64) -> i64 {
    SyscallHandler::new().handle(syscall_number, arg1, arg2, arg3)
}