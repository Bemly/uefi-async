use core::alloc::{GlobalAlloc, Layout};
use talc::{ClaimOnOom, Span, Talc, Talck};
use uefi::boot::{allocate_pages, AllocateType, MemoryType};

pub struct UefiTalc(Talck<spin::Mutex<()>, ClaimOnOom>);

#[global_allocator]
pub static ALLOCATOR: UefiTalc = unsafe {
    UefiTalc(Talc::new(ClaimOnOom::new(Span::empty())).lock::<spin::Mutex<()>>())
};

unsafe impl GlobalAlloc for UefiTalc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 { unsafe { self.0.alloc(layout) } }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) { unsafe { self.0.dealloc(ptr, layout) } }
}

impl UefiTalc {
    pub unsafe fn init(&self, start: *mut u8, size: usize) {
        let span = Span::from_base_size(start, size);

        let mut talc = self.0.lock();
        let _ = unsafe { talc.claim(span) };
    }

    /// 按页数初始化（`init` 的参数化版本；页固定 4KiB）。
    ///
    /// # Safety
    /// 与 [`init`](Self::init) 同：在首次堆分配前调用一次。
    pub unsafe fn init_with_pages(&self, pages: usize) {
        let pool_ptr = allocate_pages(AllocateType::AnyPages, MemoryType::LOADER_DATA, pages)
            .expect("Failed to allocate heap pages");
        unsafe {
            self.init(pool_ptr.as_ptr(), pages * 4096);
        }
    }
}

/// 默认堆页数（64MiB）：任务节点 + 通道都是小对象，这个量级绰绰有余；
/// 1GiB 的 QEMU 机上不再一次性钉死半壁内存。有更大需求的直接调
/// [`UefiTalc::init_with_pages`]（QEMU 里验证过，按需加到此数）。
pub const DEFAULT_HEAP_PAGES: usize = 16384; // 64MiB

/// 向后兼容的默认初始化（64MiB，见 [`DEFAULT_HEAP_PAGES`]）。
pub fn alloc_init_wrapper() {
    unsafe {
        ALLOCATOR.init_with_pages(DEFAULT_HEAP_PAGES);
    }
}