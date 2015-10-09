use memory::paging::{self, Page, Mapper};
use memory::frame_allocator::{FrameAllocator, DynamicFrameStack};
use core::iter::range_inclusive;
use rlibc::memcpy;
use spin::Mutex;

static ALLOCATOR: Mutex<Option<Allocator>> = Mutex::new(None);

const HEAD_BOTTOM: usize =  0o_001_000_000_000_0000;

struct Allocator {
    heap_top: usize,
    last_mapped_page: Page,
    lock: paging::Lock,
    frame_stack: DynamicFrameStack,
}

impl Allocator {
    pub fn allocate(&mut self, size: usize, align: usize) -> *mut u8 {
        println!("allocate {} bytes (align {})", size, align); //loop{}
        let start_address = align_up(self.heap_top, align);
        let end_address = start_address + size;
        let end_page = Page::containing_address(end_address - 1).number;
        let last_mapped_page = self.last_mapped_page.number;

        if end_page > last_mapped_page {
            for page in range_inclusive(last_mapped_page + 1, end_page).map(|n| Page{number: n}) {
                let mut mapper = self.lock.mapper(&mut self.frame_stack);
                mapper.map(page, true, false)
            }
            self.last_mapped_page.number = end_page;
        }
        self.heap_top = end_address;
        start_address as *mut u8
    }

    pub fn reallocate(&mut self, ptr: *mut u8, old_size: usize, size: usize,
        align: usize) -> *mut u8
    {
        let new_ptr = self.allocate(size, align);
        unsafe{ memcpy(new_ptr, ptr, old_size) };
        new_ptr
    }

    pub fn deallocate(&mut self, ptr: *mut u8, old_size: usize, align: usize) {
        //TODO
    }
}

fn align_up(addr: usize, align: usize) -> usize {
    if addr % align == 0 {
        addr
    } else {
        addr + align - (addr % align)
    }
}

pub fn init(mut lock: paging::Lock, mut frame_stack: DynamicFrameStack) {
    let last_mapped_page = Page::containing_address(HEAD_BOTTOM);
    {
        let mut mapper = lock.mapper(&mut frame_stack);
        mapper.map(last_mapped_page, true, false);
    }

    *ALLOCATOR.lock() = Some(Allocator {
        heap_top: HEAD_BOTTOM,
        last_mapped_page: last_mapped_page,
        lock: lock,
        frame_stack: frame_stack,
    })
}

#[no_mangle]
pub extern fn __rust_allocate(size: usize, align: usize) -> *mut u8 {
    ALLOCATOR.lock().as_mut().expect("no allocator").allocate(size, align)
}

#[no_mangle]
pub extern fn __rust_deallocate(ptr: *mut u8, old_size: usize, align: usize) {
    ALLOCATOR.lock().as_mut().expect("no allocator").deallocate(ptr, old_size, align)
}
