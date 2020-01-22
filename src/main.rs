#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(blog_os::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

use alloc::{boxed::Box, rc::Rc, vec, vec::Vec};
use blog_os::{print, println};
use bootloader::{entry_point, BootInfo};
use core::panic::PanicInfo;
use x86_64::structures::paging::{FrameAllocator, Mapper, Size4KiB};

entry_point!(kernel_main);

fn kernel_main(boot_info: &'static BootInfo) -> ! {
    use blog_os::allocator;
    use blog_os::memory::{self, BootInfoFrameAllocator};
    use x86_64::VirtAddr;

    println!("Hello World{}", "!");
    blog_os::init();

    let phys_mem_offset = VirtAddr::new(boot_info.physical_memory_offset);
    let mut mapper = unsafe { memory::init(phys_mem_offset) };
    let mut frame_allocator = unsafe { BootInfoFrameAllocator::init(&boot_info.memory_map) };

    allocator::init_heap(&mut mapper, &mut frame_allocator).expect("heap initialization failed");

    // allocate a number on the heap
    let heap_value = Box::new(41);
    println!("heap_value at {:p}", heap_value);

    // create a dynamically sized vector
    let mut vec = Vec::new();
    for i in 0..500 {
        vec.push(i);
    }
    println!("vec at {:p}", vec.as_slice());

    // create a reference counted vector -> will be freed when count reaches 0
    let reference_counted = Rc::new(vec![1, 2, 3]);
    let cloned_reference = reference_counted.clone();
    println!(
        "current reference count is {}",
        Rc::strong_count(&cloned_reference)
    );
    core::mem::drop(reference_counted);
    println!(
        "reference count is {} now",
        Rc::strong_count(&cloned_reference)
    );

    #[cfg(test)]
    test_main();

    create_thread(thread_1, &mut mapper, &mut frame_allocator);
    create_thread(thread_2, &mut mapper, &mut frame_allocator);

    println!("It did not crash!");
    blog_os::hlt_loop();
}

fn create_thread(
    f: fn() -> !,
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) {
    let mut stack = blog_os::memory::alloc_stack(1, mapper, frame_allocator).unwrap();
    stack -= core::mem::size_of::<u64>();
    let ptr: *mut u64 = stack.as_mut_ptr();
    unsafe { ptr.write(f as u64) };
    stack -= core::mem::size_of::<u64>();
    let ptr: *mut u64 = stack.as_mut_ptr();
    let rflags = 0x200;
    unsafe { ptr.write(rflags) };
    unsafe {
        blog_os::multitasking::add_thread(stack);
    }
}

fn thread_1() -> ! {
    loop {
        print!("1");
        x86_64::instructions::hlt();
    }
}

fn thread_2() -> ! {
    loop {
        print!("2");
        x86_64::instructions::hlt();
    }
}

/// This function is called on panic.
#[cfg(not(test))]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("{}", info);
    blog_os::hlt_loop();
}

#[cfg(test)]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    blog_os::test_panic_handler(info)
}
