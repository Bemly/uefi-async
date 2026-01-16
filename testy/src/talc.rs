use core::alloc::{GlobalAlloc, Layout};
use talc::{ClaimOnOom, Span, Talc, Talck};
use uefi::boot::{allocate_pages, AllocateType, MemoryType};

pub struct UefiTalc(Talck<spin::Mutex<()>, ClaimOnOom>);

#[global_allocator]
pub static ALLOCATOR: UefiTalc = unsafe {
    UefiTalc(Talc::new(ClaimOnOom::new(Span::empty())).lock::<spin::Mutex<()>>())
};

unsafe impl GlobalAlloc for UefiTalc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe { self.0.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { self.0.dealloc(ptr, layout) }
    }
}

impl UefiTalc {
    pub unsafe fn init(&self, start: *mut u8, size: usize) {
        let span = Span::from_base_size(start, size);

        let mut talc = self.0.lock();
        let _ = talc.claim(span);
    }
}

pub fn alloc_init_wrapper() {
    let pages = 131072; // 512MB
    let pool_ptr = allocate_pages(AllocateType::AnyPages, MemoryType::LOADER_DATA, pages)
        .expect("Failed to allocate heap pages");

    unsafe {
        // 初始化全局分配器
        ALLOCATOR.init(pool_ptr.as_ptr(), pages * 4096);
    }
}