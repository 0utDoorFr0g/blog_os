+++
title = "Testing"
weight = 4
path = "testing"
date  = 0000-01-01

+++

This post explores unit and integration testing in `no_std` executables. We will use Rust's support for custom test frameworks to execute test functions inside our kernel. To report the results out of QEMU, we will use different features of QEMU and the `bootimage` tool.

<!-- more -->

This blog is openly developed on [GitHub]. If you have any problems or questions, please open an issue there. You can also leave comments [at the bottom]. The complete source code for this post can be found in the [`post-04`][post branch] branch.

[GitHub]: https://github.com/phil-opp/blog_os
[at the bottom]: #comments
[post branch]: https://github.com/phil-opp/blog_os/tree/post-04

<!-- toc -->

## Requirements

This post assumes that you have a `.cargo/config` file with a default target... TODO

Earlier posts since XX, bootimage runner

## Testing in Rust

Rust has a [built-in test framework] that is capable of running unit tests without the need to set anything up. Just create a function that checks some results through assertions and add the `#[test]` attribute to the function header. Then `cargo test` will automatically find and execute all test functions of your crate.

[built-in test framework]: https://doc.rust-lang.org/book/second-edition/ch11-00-testing.html

Unfortunately it's a bit more complicated for `no_std` applications such as our kernel. The problem is that Rust's test framework implicitly uses the built-in [`test`] library, which depends on the standard library. This means that we can't use the default test framework for our `#[no_std]` kernel.

[`test`]: https://doc.rust-lang.org/test/index.html

We can see this when we try to run `cargo xtest` in our project:

```
> cargo xtest
   Compiling blog_os v0.1.0 (/home/philipp/Documents/blog_os/code)
error[E0463]: can't find crate for `test`
```

Since the `test` crate depends on the standard library, it is not available for our bare metal target. While porting the `test` crate to a `#[no_std]` context [is possible][utest], it is highly unstable and requires some hacks such as redefining the `panic` macro.

[utest]: https://github.com/japaric/utest

### Custom Test Frameworks

Fortunately, Rust supports replacing the default test framework through the unstable [`custom_test_frameworks`] feature. This feature requires no external libraries and thus also works in `#[no_std]` environments. It works by collecting all functions annotated with a `#[test_case]` attribute and then invoking a specified runner function with the list of tests as argument. Thus it gives the implementation maximal control over the test process.

[`custom_test_frameworks`]: https://doc.rust-lang.org/unstable-book/language-features/custom-test-frameworks.html

The disadvantage compared to the default test framework is that many advanced features such as [`should_panic` tests] are not available. Instead, it is up to the implementation to provide such features itself if needed. This is ideal for us since we have a very special execution environment where the default implementations of such advanced features probably wouldn't work anyway. For example, the `#[should_panic]` attribute relies on stack unwinding to catch the panics, which we disabled for our kernel.

[`should_panic` tests]: https://doc.rust-lang.org/book/ch11-01-writing-tests.html#checking-for-panics-with-should_panic

To implement a custom test framework for our kernel, we add the following to our `main.rs`:

```rust
// in src/main.rs

#![feature(custom_test_frameworks)]
#![test_runner(crate::test_runner)]

fn test_runner(tests: &[&dyn Fn()]) {
    println!("Running {} tests", tests.len());
    for test in tests {
        test();
    }
}
```

Our runner just prints a short debug message and then calls each test function in the list. The argument type `&[&dyn Fn()]` is a [_slice_] of [_trait object_] references of the [_Fn()_] trait. It is basically a list of references to types that can be called like a function.

[_slice_]: https://doc.rust-lang.org/std/primitive.slice.html
[_trait object_]: https://doc.rust-lang.org/1.30.0/book/first-edition/trait-objects.html
[_Fn()_]: https://doc.rust-lang.org/std/ops/trait.Fn.html

When we run `cargo xtest` now, we see that it now succeeds. However, we still see our "Hello World" instead of the message from our `test_runner`:

TODO image

The reason is that our `_start` function is still used as entry point. The custom test frameworks feature generates a `main` function that calls `test_runner`, but this function is ignored because we use the `#[no_main]` attribute and provide our own entry point.

To fix this, we first need to change the name of the generated function to something different than `main` through the `reexport_test_harness_main` attribute. Then we can call the renamed function from our `_start` function:

```rust
// in src/main.rs

#![reexport_test_harness_main = "test_main"]

#[no_mangle]
pub extern "C" fn _start() -> ! {
    println!("Hello World{}", "!");

    #[cfg(test)]
    test_main();

    loop {}
}
```

We set the name of the test framework entry function to `test_main` and call it from our `_start` entry point. We use [conditional compilation] to add the call to `test_main` only in test contexts because the function is not generated on a normal run.

When we now execute `cargo xtest`, we see the message from our `test_runner` on the screen:

TODO image

We are now ready to create our first test function:

```rust
// in src/main.rs

#[test_case]
fn trivial_assertion() {
    print!("trivial assertion... ");
    assert_eq!(1, 1);
    println("[ok]");
}
```

Of course the test succeeds and we see the `trivial assertion... [ok]` output on the screen. The problem is that QEMU never exits so that `cargo xtest` runs forever.

## Exiting QEMU

Right now we have an endless loop at the end of our `_start` function and need to close QEMU manually. The clean solution to this would be to implement a proper way to shutdown our OS. Unfortunatly this is relatively complex, because it requires implementing support for either the [APM] or [ACPI] power management standard.

[APM]: https://wiki.osdev.org/APM
[ACPI]: https://wiki.osdev.org/ACPI

Luckily, there is an escape hatch: QEMU supports a special `isa-debug-exit` device, which provides an easy way to exit QEMU from the guest system. To enable it, we need to pass a `-device` argument to QEMU. We can do so by adding a `package.metadata.bootimage.test-args` configuration key in our `Cargo.toml`:

```toml
# in Cargo.toml

[package.metadata.bootimage]
test-args = ["-device", "isa-debug-exit,iobase=0xf4,iosize=0x04"]
```

The `bootimage runner` appends the `test-args` to the default QEMU command for all test executables. For a normal `cargo xrun`, the arguments are ignored.

Together with the device name (`isa-debug-exit`), we pass the two parameters `iobase` and `iozize` that specify the _I/O port_ through which the device can be reached from our kernel.

### I/O Ports

There are two different approaches for communicating between the CPU and peripheral hardware on x86, **memory-mapped I/O** and **port-mapped I/O**. We already used memory-mapped I/O for accessing the [VGA text buffer] through the memory address `0xb8000`. This address is not mapped to RAM, but to some memory on the VGA device.

[VGA text buffer]: ./second-edition/posts/03-vga-text-buffer/index.md

In contrast, port-mapped I/O uses a separate I/O bus for communication. Each connected peripheral has one or more port numbers. To communicate with such an I/O port there are special CPU instructions called `in` and `out`, which take a port number and a data byte (there are also variations of these commands that allow sending an `u16` or `u32`).

The `isa-debug-exit` devices uses port-mapped I/O. The `iobase` parameter specifies on which port address the device should live (`0xf4` is a [generally unused][list of x86 I/O ports] port on the x86's IO bus) and the `iosize` specifies the port size (`0x04` means four bytes).

[list of x86 I/O ports]: https://wiki.osdev.org/I/O_Ports#The_list

### Using the Exit Device

The functionality of the `isa-debug-exit` device is very simple. When a `value` is written to the I/O port specified by `iobase`, it causes QEMU to exit with [exit status] `(value << 1) | 1`. So when we write `0` to the port QEMU will exit with exit status `(0 << 1) | 1 = 1` and when we write `1` to the port it will exit with exit status `(1 << 1) | 1 = 3`.

[exit status]: https://en.wikipedia.org/wiki/Exit_status

Instead of manually invoking the `in` and `out` assembly instructions, we use the abstractions provided by the [`x86_64`] crate. To add a dependency on that crate, we add it to the `dependencies` section in our `Cargo.toml`:

[`x86_64`]: https://docs.rs/x86_64/0.5.2/x86_64/

```toml
# in Cargo.toml

[dependencies]
x86_64 = "0.5.2"
```

Now we can use the [`Port`] type provided by the crate to create an `exit_qemu` function:

[`Port`]: https://docs.rs/x86_64/0.5.2/x86_64/instructions/port/struct.Port.html

```rust
// in src/main.rs

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum QemuExitCode {
    Success = 0x10,
    Failed = 0x11,
}

pub unsafe fn exit_qemu(exit_code: QemuExitCode) {
    use x86_64::instructions::port::Port;

    let mut port = Port::new(0xf4);
    port.write(exit_code as u32);
}
```

We mark the function as `unsafe` because it relies on the fact that a special QEMU device is attached to the I/O port with address `0xf4`. The function creates a new [`Port`] at `0xf4`, which is the `iobase` of the `isa-debug-exit` device. Then it writes the the passed exit code to the port. We use `u32` because we specified the `iosize` of the `isa-debug-exit` device as 4 bytes.

For specifying the exit status, we create a `QemuExitCode` enum. The idea is to exit with the success exit code if all tests succeeded and with the failure exit code otherwise. The enum is marked as `#[repr(u32)]` to represent each variant by an `u32` integer. We use exit code `0x10` for success and `0x11` for failure. The actual exit codes do not matter much, as long as they don't clash with the default exit codes of QEMU. For example, using exit code `0` for success is not a good idea because it becomes `(0 << 1) | 1 = 1` after the transformation, which is the default exit code when QEMU failed to run. So we could not differentiate a QEMU error from a successfull test run.

We can now update our `test_runner` to exit QEMU after all tests ran:

```rust
fn test_runner(tests: &[&dyn Fn()]) {
    println!("Running {} tests", tests.len());
    for test in tests {
        test();
    }
    /// new
    unsafe { exit_qemu(QemuExitCode::Success) };
}
```

When we run `cargo xtest` now, we see that QEMU immediately closes after executing the tests. The problem is that `cargo test` interprets the test as failed even though we passed our `Success` exit code:

```
> cargo xtest
    Finished dev [unoptimized + debuginfo] target(s) in 0.03s
     Running target/x86_64-blog_os/debug/deps/blog_os-5804fc7d2dd4c9be
Building bootloader
   Compiling bootloader v0.5.3 (/home/philipp/Documents/bootloader)
    Finished release [optimized + debuginfo] target(s) in 1.07s
Running: `qemu-system-x86_64 -drive format=raw,file=target/x86_64-blog_os/debug/
    deps/bootimage-blog_os-5804fc7d2dd4c9be.bin -device isa-debug-exit,iobase=0xf4,
    iosize=0x04`
error: test failed
```

The problem is that `cargo test` considers all error codes other than `0` as failure.

### Success Exit Code

To work around this, `bootimage` provides a `test-success-exit-code` configuration key that maps a specified exit code to the exit code `0`:

```toml
[package.metadata.bootimage]
test-args = […]
test-success-exit-code = 33         # (0x10 << 1) | 1
```

With this configuration, `bootimage` maps our success exit code to exit code 0, so that `cargo xtest` correctly recognizes the success case and does no count the test as failed.

Our test runner now automatically closes QEMU and correctly reports the test results out. We still see the QEMU window open for a very short time, but it does not suffice to read the results. It would be nice if we could print the test results to the console instead, so that we can still see them after QEMU exited.

## Printing to the Console

To see the test output on the console, we need to send the data from our kernel to the host system somehow. There are various ways to achieve this, for example by sending the data over a TCP network interface. However, setting up a networking stack is a quite complex task, so we will choose a simpler solution instead.

### Serial Port

A simple way to send the data is to use the [serial port], an old interface standard which is no longer found in modern computers. It is easy to program and QEMU can redirect the bytes sent over serial to the host's standard output or a file.

[serial port]: https://en.wikipedia.org/wiki/Serial_port

The chips implementing a serial interface are called [UARTs]. There are [lots of UART models] on x86, but fortunately the only differences between them are some advanced features we don't need. The common UARTs today are all compatible to the [16550 UART], so we will use that model for our testing framework.

[UARTs]: https://en.wikipedia.org/wiki/Universal_asynchronous_receiver-transmitter
[lots of UART models]: https://en.wikipedia.org/wiki/Universal_asynchronous_receiver-transmitter#UART_models
[16550 UART]: https://en.wikipedia.org/wiki/16550_UART

We will use the [`uart_16550`] crate to initialize the UART and send data over the serial port. To add it as a dependency, we update our `Cargo.toml` and `main.rs`:

[`uart_16550`]: https://docs.rs/uart_16550

```toml
# in Cargo.toml

[dependencies]
uart_16550 = "0.2.0"
```

The `uart_16550` crate contains a `SerialPort` struct that represents the UART registers, but we still need to construct an instance of it ourselves. For that we create a new `serial` module with the following content:

```rust
// in src/main.rs

mod serial;
```

```rust
// in src/serial.rs

use uart_16550::SerialPort;
use spin::Mutex;
use lazy_static::lazy_static;

lazy_static! {
    pub static ref SERIAL1: Mutex<SerialPort> = {
        let mut serial_port = unsafe { SerialPort::new(0x3F8) };
        serial_port.init();
        Mutex::new(serial_port)
    };
}
```

Like with the [VGA text buffer][vga lazy-static], we use `lazy_static` and a spinlock to create a `static`. However, this time we use `lazy_static` to ensure that the `init` method is called before first use. We're using the port address `0x3F8`, which is the standard port number for the first serial interface.

[vga lazy-static]: ./second-edition/posts/03-vga-text-buffer/index.md#lazy-statics

To make the serial port easily usable, we add `serial_print!` and `serial_println!` macros:

```rust
#[doc(hidden)]
pub fn _print(args: ::core::fmt::Arguments) {
    use core::fmt::Write;
    SERIAL1.lock().write_fmt(args).expect("Printing to serial failed");
}

/// Prints to the host through the serial interface.
#[macro_export]
macro_rules! serial_print {
    ($($arg:tt)*) => {
        $crate::serial::_print(format_args!($($arg)*));
    };
}

/// Prints to the host through the serial interface, appending a newline.
#[macro_export]
macro_rules! serial_println {
    () => ($crate::serial_print!("\n"));
    ($fmt:expr) => ($crate::serial_print!(concat!($fmt, "\n")));
    ($fmt:expr, $($arg:tt)*) => ($crate::serial_print!(
        concat!($fmt, "\n"), $($arg)*));
}
```

The `SerialPort` type already implements the [`fmt::Write`] trait, so we don't need to provide an implementation.

[`fmt::Write`]: https://doc.rust-lang.org/nightly/core/fmt/trait.Write.html

Now we can print to the serial interface instead of the VGA text buffer in our test code:

```rust
// in src/main.rs

```rust
fn test_runner(tests: &[&dyn Fn()]) {
    serial_println!("Running {} tests", tests.len());
    […]
}

#[test_case]
fn trivial_assertion() {
    serial_print!("trivial assertion... ");
    assert_eq!(1, 1);
    serial_println("[ok]");
}
```

Note that the `serial_println` macro lives directly under the root namespace because we used the `#[macro_export]` attribute, so importing it through `use crate::serial::serial_println` will not work.

### QEMU Arguments

To see the serial output in QEMU, we need use the `-serial` argument to redirect the output to stdout:

```toml
# in Cargo.toml

[package.metadata.bootimage]
test-args = [
    "-device", "isa-debug-exit,iobase=0xf4,iosize=0x04", "-serial", "mon:stdio"
]
```

When we run `cargo xtest` now, we see the test output directly in the console:

```
> cargo xtest
TODO
```

However, when a test fails we still see the output inside QEMU because our panic handler still uses `println`. To simulate this, we can change the assertion in our `trivial_assertion` test to `assert_eq!(0, 1)`:

TODO image

Note that it's no longer possible to exit QEMU from the console through `Ctrl+c` when `serial mon:stdio` is passed. An alternative keyboard shortcut is `Ctrl+a` and then `x`. Or you can just close the QEMU window manually.

### Print a Error Message on Panic

To exit QEMU with an error message on a panic, we can use [conditional compilation] to use a different panic handler in testing mode:

[conditional compilation]: https://doc.rust-lang.org/1.30.0/book/first-edition/conditional-compilation.html

```rust
// our existing panic handler
#[cfg(not(test))] // new attribute
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("{}", info);
    loop {}
}

// our panic handler in test mode
#[cfg(test)]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    serial_println!("[failed]\n");
    serial_println!("Error: {}\n", info);
    unsafe { exit_qemu(QemuExitCode::Failed); }
    loop {}
}
```

For our test panic handler, we use `serial_println` instead of `println` and then exit QEMU with a failure exit code. Note that we still need an endless `loop` after the `exit_qemu` call because the compiler does not know that the `isa-debug-exit` device causes a program exit.

Now QEMU also exits for failed tests and prints a useful error message on the console:

```
> cargo xtest
TODO
```

We still see the QEMU window open for a short time, which we don't need anymore.

### Hiding QEMU

Since we report out the complete test results using the `isa-debug-exit` device and the serial port, we don't need the QEMU window anymore. We can hide it by passing the `-display none` argument to QEMU:

```toml
# in Cargo.toml

[package.metadata.bootimage]
test-args = [
    "-device", "isa-debug-exit,iobase=0xf4,iosize=0x04", "-serial", "mon:stdio",
    "-display", "none"
]
```

Now QEMU runs completely in the background and no window is opened anymore. This is not only less annoying, but also allows our test framework to run in environments without a graphical user interface, such as CI services or [SSH] connections.

[SSH]: https://en.wikipedia.org/wiki/Secure_Shell

## Testing the VGA Buffer

Now that we have a working test framework, we can create a few tests for our VGA buffer implementation. First, we create a very simple test to verify that `println` works without panicking:

```rust
// in src/vga_buffer.rs

#[test_case]
fn test_println_simple() {
    serial_print!("test_println... ");
    println!("test_println_simple output");
    serial_println("[ok]");
}
```

The test just prints something to the VGA buffer. If it finishes without panicking, it means that the `println` invocation did not panic either.

To ensure that no panic occurs even if many lines are printed and lines are shifted off the screen, we can create another test:

```rust
// in src/vga_buffer.rs

#[test_case]
fn test_println_many() {
    serial_print!("test_println... ");
    for _ in 0..1000 {
        println!("test_println_many output");
    }
    serial_println("[ok]");
}
```

We can also create a test function to verify that the printed lines really appear on the screen:

```rust
// in src/vga_buffer.rs

#[test_case]
fn check_println_output() {
    serial_print!("test_println... ");

    let s = "Some test string that fits on a single line";
    println!("{}", s);
    for (i, c) in s.chars().enumerate() {
        let screen_char = WRITER.lock().chars[BUFFER_HEIGHT - 2][i].load();
        assert_eq!(char::from(screen_char.ascii_character), c);
    }

    serial_println("[ok]");
}
```

The function defines a test string, prints it using `println`, and then iterates over the screen characters of the static `WRITER`, which represents the vga text buffer. Since `println` prints to the last screen line and then immediately appends a newline, the string should appear on line `BUFFER_HEIGHT - 2`.

By using [`enumerate`], we count the number of iterations in the variable `i`, which we then use for loading the screen character corresponding to `c`. By comparing the `ascii_character` of the screen character with `c`, we ensure that each character of the string really appears in the vga text buffer.

[`enumerate`]: https://doc.rust-lang.org/core/iter/trait.Iterator.html#method.enumerate

As you can imagine, we could create many more test functions, for example a function that tests that no panic occurs when printing very long lines and that they're wrapped correctly. Or a function for testing that newlines, non-printable characters, and non-unicode charactes are handled correctly.

For the rest of this post, however, we will explain how to create _integration tests_ to test the interaction of different components together.

## Integration Tests

The convention for [integration tests] in Rust is to put them into a `tests` directory in the project root (i.e. next to the `src` directory). Both the default test framework and custom test frameworks will automatically pick up and execute all tests in that directory.

[integration tests]: https://doc.rust-lang.org/book/ch11-03-test-organization.html#integration-tests

All integration tests are their own executables and completely separate from our `main.rs`. This means that each test needs to define its own entry point function. Let's create an example integration test named `basic_boot` to see how it works in detail:

```rust
// in tests/basic_boot.rs

#![no_std]
#![no_main]

#![feature(custom_test_frameworks)]
#![test_runner(blog_os::test_runner)]
#![reexport_test_harness_main = "test_main"]

use core::panic::PanicInfo;

#[no_mangle] // don't mangle the name of this function
pub extern "C" fn _start() -> ! {
    test_main();

    loop {}
}

fn test_runner(tests: &[&dyn Fn()]) {
    unimplemented!();
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    unimplemented!();
}
```

Since integration tests are separate executables we need to provide all the crate attributes (`no_std`, `no_main`, `test_runner`, etc.) again. We also need to create a new entry point function `_start`, which calls the test entry point function `test_main`. We don't need any `cfg(test)` attributes because integration test executables are never built in non-test mode.

We use the [`unimplemented`] macro that always panics as a placeholder for the `test_runner` and the `panic` function. Ideally, we want to implement these functions exactly as we did in our `main.rs` using the `serial_println` macro and the `exit_qemu` function. The problem is that we don't have access to these functions since tests are built completely separately of our `main.rs` executable.

[`unimplemented`]: https://doc.rust-lang.org/core/macro.unimplemented.html

### Create a Library

The solution is to split off a library from our `main.rs`, which can be included by other crates and integration test executables. To do this, we create a new `src/lib.rs` file and move most of our `main.rs` to it:

```rust
// src/lib.rs

TODO
```

As you can see, we moved all module declarations, the `exit_qemu` function, and our test runner into the library. Since the test runner also runs for our library, it needs to define its own entry point function in test-mode. To share the implementation of the entry point between our `main.rs` and our `lib.rs`, we move it into a new `run` function.

The remaining code of our `src/main.rs` is:

```rust
// src/main.rs

TODO
```

We add a new `use` statement that imports all the used functions and macros from our library component. The library is called like your crate, which is named `blog_os` in our case. From the `_start` entry point we call the `run` function of our `lib.rs` to use the same environment for our `main.rs` and our `lib.rs` tests.

### Completing the Integration Test

Like our `src/main.rs`, our `tests/basic_boot.rs` executable can import types from our new library. This allows us to import the missing components to complete our test.

```rust
// in tests/basic_boot.rs

TODO
```

Instead of reimplementing the `test_runner`, we use the `test_runner` function of the library. We deliberatly don't call the `run` function from `_start` to let the tests run in a minimal boot environment. This way, we can test that certain features don't depend on initialization code that we will add to our `run` function in future posts.

For example, we can test that `println` works right after boot without needing any initialization:

```rust
TODO
```

This test is very similar to the TODO vga buffer test that we created earlier in this post. The important difference is that TODO runs at the end of the `run` function and TODO runs directly after boot without running any initialization code beforehand.

At this stage, a test like this doesn't seem very useful. However, when our kernel becomes more featureful in the future, integration tests like this will be useful for testing certain features in well defined environments. For example, we might want to prepare certain page table mappings and that are used in our tests.

### Testing Our Panic Handler

Another thing that we can test with an integration test is our panic handler function. The idea is the following:

- Deliberately cause a panic in the test
- Add assertions in the panic handler that check the panic message and the file/line information
- Exit with a success exit code at the end of the panic handler

This is similar to a should panic test in the default Rust test framework. The difference is that can't continue the test after our panic handler was called because we don't have support for unwinding and the catch_panic function.

For cases like this, where more than a single test are not useful, we can use the `no harness` feature to omit the test runner completely.

#### No Harness


# Unit Tests

## Testing the VGA Module
Now that we have set up the test framework, we can add a first unit test for our `vga_buffer` module:

```rust
// in src/vga_buffer.rs

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn foo() {}
}
```

We add the test in an inline `test` submodule. This isn't necessary, but a common way to separate test code from the rest of the module. By adding the `#[cfg(test)]` attribute, we ensure that the module is only compiled in test mode. Through `use super::*`, we import all items of the parent module (the `vga_buffer` module), so that we can test them easily.

The `#[test]` attribute on the `foo` function tells the test framework that the function is an unit test. The framework will find it automatically, even if it's private and inside a private module as in our case:

```
> cargo test
   Compiling blog_os v0.2.0 (file:///…/blog_os)
    Finished dev [unoptimized + debuginfo] target(s) in 2.99 secs
     Running target/debug/deps/blog_os-1f08396a9eff0aa7

running 1 test
test vga_buffer::test::foo ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

We see that the test was found and executed. It didn't panic, so it counts as passed.

### Constructing a Writer
In order to test the VGA methods, we first need to construct a `Writer` instance. Since we will need such an instance for other tests too, we create a separate function for it:

```rust
// in src/vga_buffer.rs

#[cfg(test)]
mod test {
    use super::*;

    fn construct_writer() -> Writer {
        use std::boxed::Box;

        let buffer = construct_buffer();
        Writer {
            column_position: 0,
            color_code: ColorCode::new(Color::Blue, Color::Magenta),
            buffer: Box::leak(Box::new(buffer)),
        }
    }

    fn construct_buffer() -> Buffer { … }
}
```

We set the initial column position to 0 and choose some arbitrary colors for foreground and background color. The difficult part is the buffer construction, it's described in detail below. We then use [`Box::new`] and [`Box::leak`] to transform the created `Buffer` into a `&'static mut Buffer`, because the `buffer` field needs to be of that type.

[`Box::new`]: https://doc.rust-lang.org/nightly/std/boxed/struct.Box.html#method.new
[`Box::leak`]: https://doc.rust-lang.org/nightly/std/boxed/struct.Box.html#method.leak

#### Buffer Construction
So how do we create a `Buffer` instance? The naive approach does not work unfortunately:

```rust
fn construct_buffer() -> Buffer {
    Buffer {
        chars: [[Volatile::new(empty_char()); BUFFER_WIDTH]; BUFFER_HEIGHT],
    }
}

fn empty_char() -> ScreenChar {
    ScreenChar {
        ascii_character: b' ',
        color_code: ColorCode::new(Color::Green, Color::Brown),
    }
}
```

When running `cargo test` the following error occurs:

```
error[E0277]: the trait bound `volatile::Volatile<vga_buffer::ScreenChar>: core::marker::Copy` is not satisfied
   --> src/vga_buffer.rs:186:21
    |
186 |             chars: [[Volatile::new(empty_char); BUFFER_WIDTH]; BUFFER_HEIGHT],
    |                     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ the trait `core::marker::Copy` is not implemented for `volatile::Volatile<vga_buffer::ScreenChar>`
    |
    = note: the `Copy` trait is required because the repeated element will be copied
```

The problem is that array construction in Rust requires that the contained type is [`Copy`]. The `ScreenChar` is `Copy`, but the `Volatile` wrapper is not. There is currently no easy way to circumvent this without using [`unsafe`], but fortunately there is the [`array_init`] crate that provides a safe interface for such operations.

[`Copy`]: https://doc.rust-lang.org/core/marker/trait.Copy.html
[`unsafe`]: https://doc.rust-lang.org/book/second-edition/ch19-01-unsafe-rust.html
[`array_init`]: https://docs.rs/array-init

To use that crate, we add the following to our `Cargo.toml`:

```toml
[dev-dependencies]
array-init = "0.0.3"
```

Note that we're using the [`dev-dependencies`] table instead of the `dependencies` table, because we only need the crate for `cargo test` and not for a normal build.

[`dev-dependencies`]: https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html#development-dependencies

Now we can fix our `construct_buffer` function:

```rust
fn construct_buffer() -> Buffer {
    use array_init::array_init;

    Buffer {
        chars: array_init(|_| array_init(|_| Volatile::new(empty_char()))),
    }
}
```

See the [documentation of `array_init`][`array_init`] for more information about using that crate.

### Testing `write_byte`
Now we're finally able to write a first unit test that tests the `write_byte` method:

```rust
// in vga_buffer.rs

mod test {
    […]

    #[test]
    fn write_byte() {
        let mut writer = construct_writer();
        writer.write_byte(b'X');
        writer.write_byte(b'Y');

        for (i, row) in writer.buffer.chars.iter().enumerate() {
            for (j, screen_char) in row.iter().enumerate() {
                let screen_char = screen_char.read();
                if i == BUFFER_HEIGHT - 1 && j == 0 {
                    assert_eq!(screen_char.ascii_character, b'X');
                    assert_eq!(screen_char.color_code, writer.color_code);
                } else if i == BUFFER_HEIGHT - 1 && j == 1 {
                    assert_eq!(screen_char.ascii_character, b'Y');
                    assert_eq!(screen_char.color_code, writer.color_code);
                } else {
                    assert_eq!(screen_char, empty_char());
                }
            }
        }
    }
}
```

We construct a `Writer`, write two bytes to it, and then check that the right screen characters were updated. When we run `cargo test`, we see that the test is executed and passes:

```
running 1 test
test vga_buffer::test::write_byte ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

Try to play around a bit with this function and verify that the test fails if you change something, e.g. if you print a third byte without adjusting the `for` loop.

(If you're getting an “binary operation `==` cannot be applied to type `vga_buffer::ScreenChar`” error, you need to also derive [`PartialEq`] for `ScreenChar` and `ColorCode`).

[`PartialEq`]: https://doc.rust-lang.org/nightly/core/cmp/trait.PartialEq.html

### Testing Strings
Let's add a second unit test to test formatted output and newline behavior:

```rust
// in src/vga_buffer.rs

mod test {
    […]

    #[test]
    fn write_formatted() {
        use core::fmt::Write;

        let mut writer = construct_writer();
        writeln!(&mut writer, "a").unwrap();
        writeln!(&mut writer, "b{}", "c").unwrap();

        for (i, row) in writer.buffer.chars.iter().enumerate() {
            for (j, screen_char) in row.iter().enumerate() {
                let screen_char = screen_char.read();
                if i == BUFFER_HEIGHT - 3 && j == 0 {
                    assert_eq!(screen_char.ascii_character, b'a');
                    assert_eq!(screen_char.color_code, writer.color_code);
                } else if i == BUFFER_HEIGHT - 2 && j == 0 {
                    assert_eq!(screen_char.ascii_character, b'b');
                    assert_eq!(screen_char.color_code, writer.color_code);
                } else if i == BUFFER_HEIGHT - 2 && j == 1 {
                    assert_eq!(screen_char.ascii_character, b'c');
                    assert_eq!(screen_char.color_code, writer.color_code);
                } else if i >= BUFFER_HEIGHT - 2 {
                    assert_eq!(screen_char.ascii_character, b' ');
                    assert_eq!(screen_char.color_code, writer.color_code);
                } else {
                    assert_eq!(screen_char, empty_char());
                }
            }
        }
    }
}
```

In this test we're using the [`writeln!`] macro to print strings with newlines to the buffer. Most of the for loop is similar to the `write_byte` test and only verifies if the written characters are at the expected place. The new `if i >= BUFFER_HEIGHT - 2` case verifies that the empty lines that are shifted in on a newline have the `writer.color_code`, which is different from the initial color.

[`writeln!`]: https://doc.rust-lang.org/nightly/core/macro.writeln.html

### More Tests
We only present two basic tests here as an example, but of course many more tests are possible. For example a test that changes the writer color in between writes. Or a test that checks that the top line is correctly shifted off the screen on a newline. Or a test that checks that non-ASCII characters are handled correctly.

## Summary
Unit testing is a very useful technique to ensure that certain components have a desired behavior. Even if they cannot show the absence of bugs, they're still an useful tool for finding them and especially for avoiding regressions.

This post explained how to set up unit testing in a Rust kernel. We now have a functioning test framework and can easily add tests by adding functions with a `#[test]` attribute. To run them, a short `cargo test` suffices. We also added a few basic tests for our VGA buffer as an example how unit tests could look like.

We also learned a bit about conditional compilation, Rust's [lint system], how to [initialize arrays with non-Copy types], and the `dev-dependencies` section of the `Cargo.toml`.

[lint system]: #silencing-the-warnings
[initialize arrays with non-Copy types]: #buffer-construction

## What's next?
We now have a working unit testing framework, which gives us the ability to test individual components. However, unit tests have the disadvantage that they run on the host machine and are thus unable to test how components interact with platform specific parts. For example, we can't test the `println!` macro with an unit test because it wants to write at the VGA text buffer at address `0xb8000`, which only exists in the bare metal environment.

The next post will close this gap by creating a basic _integration test_ framework, which runs the tests in QEMU and thus has access to platform specific components. This will allow us to test the full system, for example that our kernel boots correctly or that no deadlock occurs on nested `println!` invocations.




# Integration Tests

## Overview

In the previous post we added support for unit tests. The goal of unit tests is to test small components in isolation to ensure that each of them works as intended. The tests are run on the host machine and thus shouldn't rely on architecture specific functionality.

To test the interaction of the components, both with each other and the system environment, we can write _integration tests_. Compared to unit tests, ìntegration tests are more complex, because they need to run in a realistic environment. What this means depends on the application type. For example, for webserver applications it often means to set up a database instance. For an operating system kernel like ours, it means that we run the tests on the target hardware without an underlying operating system.

Running on the target architecture allows us to test all hardware specific code such as the VGA buffer or the effects of [page table] modifications. It also allows us to verify that our kernel boots without problems and that no [CPU exception] occurs.

[page table]: https://en.wikipedia.org/wiki/Page_table
[CPU exception]: https://wiki.osdev.org/Exceptions

In this post we will implement a very basic test framework that runs integration tests inside instances of the [QEMU] virtual machine. It is not as realistic as running them on real hardware, but it is much simpler and should be sufficient as long as we only use standard hardware that is well supported in QEMU.

[QEMU]: https://www.qemu.org/

## The Serial Port

The naive way of doing an integration test would be to add some assertions in the code, launch QEMU, and manually check if a panic occured or not. This is very cumbersome and not practical if we have hundreds of integration tests. So we want an automated solution that runs all tests and fails if not all of them pass.

Such an automated test framework needs to know whether a test succeeded or failed. It can't look at the screen output of QEMU, so we need a different way of retrieving the test results on the host system. A simple way to achieve this is by using the [serial port], an old interface standard which is no longer found in modern computers. It is easy to program and QEMU can redirect the bytes sent over serial to the host's standard output or a file.

[serial port]: https://en.wikipedia.org/wiki/Serial_port

The chips implementing a serial interface are called [UARTs]. There are [lots of UART models] on x86, but fortunately the only differences between them are some advanced features we don't need. The common UARTs today are all compatible to the [16550 UART], so we will use that model for our testing framework.

[UARTs]: https://en.wikipedia.org/wiki/Universal_asynchronous_receiver-transmitter
[lots of UART models]: https://en.wikipedia.org/wiki/Universal_asynchronous_receiver-transmitter#UART_models
[16550 UART]: https://en.wikipedia.org/wiki/16550_UART

### Port I/O
There are two different approaches for communicating between the CPU and peripheral hardware on x86, **memory-mapped I/O** and **port-mapped I/O**. We already used memory-mapped I/O for accessing the [VGA text buffer] through the memory address `0xb8000`. This address is not mapped to RAM, but to some memory on the GPU.

[VGA text buffer]: ./second-edition/posts/03-vga-text-buffer/index.md

In contrast, port-mapped I/O uses a separate I/O bus for communication. Each connected peripheral has one or more port numbers. To communicate with such an I/O port there are special CPU instructions called `in` and `out`, which take a port number and a data byte (there are also variations of these commands that allow sending an `u16` or `u32`).

The UART uses port-mapped I/O. Fortunately there are already several crates that provide abstractions for I/O ports and even UARTs, so we don't need to invoke the `in` and `out` assembly instructions manually.

### Implementation

We will use the [`uart_16550`] crate to initialize the UART and send data over the serial port. To add it as a dependency, we update our `Cargo.toml` and `main.rs`:

[`uart_16550`]: https://docs.rs/uart_16550

```toml
# in Cargo.toml

[dependencies]
uart_16550 = "0.1.0"
```

The `uart_16550` crate contains a `SerialPort` struct that represents the UART registers, but we still need to construct an instance of it ourselves. For that we create a new `serial` module with the following content:

```rust
// in src/main.rs

mod serial;
```

```rust
// in src/serial.rs

use uart_16550::SerialPort;
use spin::Mutex;
use lazy_static::lazy_static;

lazy_static! {
    pub static ref SERIAL1: Mutex<SerialPort> = {
        let mut serial_port = SerialPort::new(0x3F8);
        serial_port.init();
        Mutex::new(serial_port)
    };
}
```

Like with the [VGA text buffer][vga lazy-static], we use `lazy_static` and a spinlock to create a `static`. However, this time we use `lazy_static` to ensure that the `init` method is called before first use. We're using the port address `0x3F8`, which is the standard port number for the first serial interface.

[vga lazy-static]: ./second-edition/posts/03-vga-text-buffer/index.md#lazy-statics

To make the serial port easily usable, we add `serial_print!` and `serial_println!` macros:

```rust
#[doc(hidden)]
pub fn _print(args: ::core::fmt::Arguments) {
    use core::fmt::Write;
    SERIAL1.lock().write_fmt(args).expect("Printing to serial failed");
}

/// Prints to the host through the serial interface.
#[macro_export]
macro_rules! serial_print {
    ($($arg:tt)*) => {
        $crate::serial::_print(format_args!($($arg)*));
    };
}

/// Prints to the host through the serial interface, appending a newline.
#[macro_export]
macro_rules! serial_println {
    () => ($crate::serial_print!("\n"));
    ($fmt:expr) => ($crate::serial_print!(concat!($fmt, "\n")));
    ($fmt:expr, $($arg:tt)*) => ($crate::serial_print!(
        concat!($fmt, "\n"), $($arg)*));
}
```

The `SerialPort` type already implements the [`fmt::Write`] trait, so we don't need to provide an implementation.

[`fmt::Write`]: https://doc.rust-lang.org/nightly/core/fmt/trait.Write.html

Now we can print to the serial interface in our `main.rs`:

```rust
// in src/main.rs

mod serial;

#[cfg(not(test))]
#[no_mangle]
pub extern "C" fn _start() -> ! {
    println!("Hello World{}", "!"); // prints to vga buffer
    serial_println!("Hello Host{}", "!");

    loop {}
}
```

Note that the `serial_println` macro lives directly under the root namespace because we used the `#[macro_export]` attribute, so importing it through `use crate::serial::serial_println` will not work.

### QEMU Arguments

To see the serial output in QEMU, we can use the `-serial` argument to redirect the output to stdout:

```
> qemu-system-x86_64 \
    -drive format=raw,file=target/x86_64-blog_os/debug/bootimage-blog_os.bin \
    -serial mon:stdio
warning: TCG doesn't support requested feature: CPUID.01H:ECX.vmx [bit 5]
Hello Host!
```

If you chose a different name than `blog_os`, you need to update the paths of course. Note that you can no longer exit QEMU through `Ctrl+c`. As an alternative you can use `Ctrl+a` and then `x`.

As an alternative to this long command, we can pass the argument to `bootimage run`, with an additional `--` to separate the build arguments (passed to cargo) from the run arguments (passed to QEMU).

```
bootimage run -- -serial mon:stdio
```

Instead of standard output, QEMU supports [many more target devices][QEMU -serial]. For redirecting the output to a file, the argument is:

[QEMU -serial]: https://qemu.weilnetz.de/doc/qemu-doc.html#Debug_002fExpert-options

```
-serial file:output-file.txt
```

## Shutting Down QEMU

Right now we have an endless loop at the end of our `_start` function and need to close QEMU manually. This does not work for automated tests. We could try to kill QEMU automatically from the host, for example after some special output was sent over serial, but this would be a bit hacky and difficult to get right. The cleaner solution would be to implement a way to shutdown our OS. Unfortunatly this is relatively complex, because it requires implementing support for either the [APM] or [ACPI] power management standard.

[APM]: https://wiki.osdev.org/APM
[ACPI]: https://wiki.osdev.org/ACPI

Luckily, there is an escape hatch: QEMU supports a special `isa-debug-exit` device, which provides an easy way to exit QEMU from the guest system. To enable it, we add the following argument to our QEMU command:

```
-device isa-debug-exit,iobase=0xf4,iosize=0x04
```

The `iobase` specifies on which port address the device should live (`0xf4` is a [generally unused][list of x86 I/O ports] port on the x86's IO bus) and the `iosize` specifies the port size (`0x04` means four bytes). Now the guest can write a value to the `0xf4` port and QEMU will exit with [exit status] `(passed_value << 1) | 1`.

[list of x86 I/O ports]: https://wiki.osdev.org/I/O_Ports#The_list
[exit status]: https://en.wikipedia.org/wiki/Exit_status

To write to the I/O port, we use the [`x86_64`] crate:

[`x86_64`]: https://docs.rs/x86_64/0.5.2/x86_64/

```toml
# in Cargo.toml

[dependencies]
x86_64 = "0.5.2"
```

```rust
// in src/main.rs

pub unsafe fn exit_qemu() {
    use x86_64::instructions::port::Port;

    let mut port = Port::<u32>::new(0xf4);
    port.write(0);
}
```

We mark the function as `unsafe` because it relies on the fact that a special QEMU device is attached to the I/O port with address `0xf4`. For the port type we choose `u32` because the `iosize` is 4 bytes. As value we write a zero, which causes QEMU to exit with exit status `(0 << 1) | 1 = 1`.

Note that we could also use the exit status instead of the serial interface for sending the test results, for example `1` for success and `2` for failure. However, this wouldn't allow us to send panic messages like the serial interface does and would also prevent us from replacing `exit_qemu` with a proper shutdown someday. Therefore we continue to use the serial interface and just always write a `0` to the port.

We can now test the QEMU shutdown by calling `exit_qemu` from our `_start` function:

```rust
#[cfg(not(test))]
#[no_mangle]
pub extern "C" fn _start() -> ! {
    println!("Hello World{}", "!"); // prints to vga buffer
    serial_println!("Hello Host{}", "!");

    unsafe { exit_qemu(); }

    loop {}
}
```

You should see that QEMU immediately closes after booting when executing:

```
bootimage run -- -serial mon:stdio -device isa-debug-exit,iobase=0xf4,iosize=0x04
```

## Hiding QEMU

We are now able to launch a QEMU instance that writes its output to the serial port and automatically exits itself when it's done. So we no longer need the VGA buffer output or the graphical representation that still pops up. We can disable it by passing the `-display none` parameter to QEMU. The full command looks like this:

```
qemu-system-x86_64 \
    -drive format=raw,file=target/x86_64-blog_os/debug/bootimage-blog_os.bin \
    -serial mon:stdio \
    -device isa-debug-exit,iobase=0xf4,iosize=0x04 \
    -display none
```

Or, with `bootimage run`:

```
bootimage run -- \
    -serial mon:stdio \
    -device isa-debug-exit,iobase=0xf4,iosize=0x04 \
    -display none
```

Now QEMU runs completely in the background and no window is opened anymore. This is not only less annoying, but also allows our test framework to run in environments without a graphical user interface, such as [Travis CI].

[Travis CI]: https://travis-ci.com/

## Test Organization

Right now we're doing the serial output and the QEMU exit from the `_start` function in our `main.rs` and can no longer run our kernel in a normal way. We could try to fix this by adding an `integration-test` [cargo feature] and using [conditional compilation]:

[cargo feature]: https://doc.rust-lang.org/cargo/reference/manifest.html#the-features-section
[conditional compilation]: https://doc.rust-lang.org/reference/attributes.html#conditional-compilation

```toml
# in Cargo.toml

[features]
integration-test = []
```

```rust
// in src/main.rs

#[cfg(not(feature = "integration-test"))] // new
#[cfg(not(test))]
#[no_mangle]
pub extern "C" fn _start() -> ! {
    println!("Hello World{}", "!"); // prints to vga buffer

    // normal execution

    loop {}
}

#[cfg(feature = "integration-test")] // new
#[cfg(not(test))]
#[no_mangle]
pub extern "C" fn _start() -> ! {
    serial_println!("Hello Host{}", "!");

    run_test_1();
    run_test_2();
    // run more tests

    unsafe { exit_qemu(); }

    loop {}
}
```

However, this approach has a big problem: All tests run in the same kernel instance, which means that they can influence each other. For example, if `run_test_1` misconfigures the system by loading an invalid [page table], it can cause `run_test_2` to fail. This isn't something that we want because it makes it very difficult to find the actual cause of an error.

[page table]: https://en.wikipedia.org/wiki/Page_table

Instead, we want our test instances to be as independent as possible. If a test wants to destroy most of the system configuration to ensure that some property still holds in catastrophic situations, it should be able to do so without needing to restore a correct system state afterwards. This means that we need to launch a separate QEMU instance for each test.

With the above conditional compilation we only have two modes: Run the kernel normally or execute _all_ integration tests. To run each test in isolation we would need a separate cargo feature for each test with that approach, which would result in very complex conditional compilation bounds and confusing code.

A better solution is to create an additional executable for each test.

### Additional Test Executables

Cargo allows to add [additional executables] to a project by putting them inside `src/bin`. We can use that feature to create a separate executable for each integration test. For example, a `test-something` executable could be added like this:

[additional executables]: https://doc.rust-lang.org/cargo/reference/manifest.html#the-project-layout

```rust
// src/bin/test-something.rs

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
#![cfg_attr(test, allow(unused_imports))]

use core::panic::PanicInfo;

#[cfg(not(test))]
#[no_mangle]
pub extern "C" fn _start() -> ! {
    // run tests
    loop {}
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}
```

By providing a new implementation for `_start` we can create a minimal test case that only tests one specific thing and is independent of the rest. For example, if we don't print anything to the VGA buffer, the test still succeeds even if the `vga_buffer` module is broken.

We can now run this executable in QEMU by passing a `--bin` argument to `bootimage`:

```
bootimage run --bin test-something
```

It should build the `test-something.rs` executable instead of `main.rs` and launch an empty QEMU window (since we don't print anything). So this approach allows us to create completely independent executables without cargo features or conditional compilation, and without cluttering our `main.rs`.

However, there is a problem: This is a completely separate executable, which means that we can't access any functions from our `main.rs`, including `serial_println` and `exit_qemu`. Duplicating the code would work, but we would also need to copy everything we want to test. This would mean that we no longer test the original function but only a possibly outdated copy.

Fortunately there is a way to share most of the code between our `main.rs` and the testing binaries: We move most of the code from our `main.rs` to a library that we can include from all executables.

### Split Off A Library

Cargo supports hybrid projects that are both a library and a binary. We only need to create a `src/lib.rs` file and split the contents of our `main.rs` in the following way:

```rust
// src/lib.rs

#![cfg_attr(not(test), no_std)] // don't link the Rust standard library

// NEW: We need to add `pub` here to make them accessible from the outside
pub mod vga_buffer;
pub mod serial;

pub unsafe fn exit_qemu() {
    use x86_64::instructions::port::Port;

    let mut port = Port::<u32>::new(0xf4);
    port.write(0);
}
```

```rust
// src/main.rs

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
#![cfg_attr(test, allow(unused_imports))]

use core::panic::PanicInfo;
use blog_os::println;

/// This function is the entry point, since the linker looks for a function
/// named `_start` by default.
#[cfg(not(test))]
#[no_mangle] // don't mangle the name of this function
pub extern "C" fn _start() -> ! {
    println!("Hello World{}", "!");

    loop {}
}

/// This function is called on panic.
#[cfg(not(test))]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("{}", info);
    loop {}
}
```

So we move everything except `_start` and `panic` to `lib.rs` and make the `vga_buffer` and `serial` modules public. Everything should work exactly as before, including `bootimage run` and `cargo test`. To run tests only for the library part of our crate and avoid the additional output we can execute `cargo test --lib`.

### Test Basic Boot

We are finally able to create our first integration test executable. We start simple and only test that the basic boot sequence works and the `_start` function is called:

```rust
// in src/bin/test-basic-boot.rs

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)] // disable all Rust-level entry points
#![cfg_attr(test, allow(unused_imports))]

use core::panic::PanicInfo;
use blog_os::{exit_qemu, serial_println};

/// This function is the entry point, since the linker looks for a function
/// named `_start` by default.
#[cfg(not(test))]
#[no_mangle] // don't mangle the name of this function
pub extern "C" fn _start() -> ! {
    serial_println!("ok");

    unsafe { exit_qemu(); }
    loop {}
}


/// This function is called on panic.
#[cfg(not(test))]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    serial_println!("failed");

    serial_println!("{}", info);

    unsafe { exit_qemu(); }
    loop {}
}
```

We don't do something special here, we just print `ok` if `_start` is called and `failed` with the panic message when a panic occurs. Let's try it:

```
> bootimage run --bin test-basic-boot -- \
    -serial mon:stdio -display none \
    -device isa-debug-exit,iobase=0xf4,iosize=0x04
Building kernel
   Compiling blog_os v0.2.0 (file:///…/blog_os)
    Finished dev [unoptimized + debuginfo] target(s) in 0.19s
    Updating registry `https://github.com/rust-lang/crates.io-index`
Creating disk image at target/x86_64-blog_os/debug/bootimage-test-basic-boot.bin
warning: TCG doesn't support requested feature: CPUID.01H:ECX.vmx [bit 5]
ok
```

We got our `ok`, so it worked! Try inserting a `panic!()` before the `ok` printing, you should see output like this:

```
failed
panicked at 'explicit panic', src/bin/test-basic-boot.rs:19:5
```

### Test Panic

To test that our panic handler is really invoked on a panic, we create a `test-panic` test:

```rust
// in src/bin/test-panic.rs

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
#![cfg_attr(test, allow(unused_imports))]

use core::panic::PanicInfo;
use blog_os::{exit_qemu, serial_println};

#[cfg(not(test))]
#[no_mangle]
pub extern "C" fn _start() -> ! {
    panic!();
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    serial_println!("ok");

    unsafe { exit_qemu(); }
    loop {}
}
```

This executable is almost identical to `test-basic-boot`, the only difference is that we print `ok` from our panic handler and invoke an explicit `panic()` in our `_start` function.

## A Test Runner

The final step is to create a test runner, a program that executes all integration tests and checks their results. The basic steps that it should do are:

- Look for integration tests in the current project, maybe by some convention (e.g. executables starting with `test-`).
- Run all integration tests and interpret their results.
    - Use a timeout to ensure that an endless loop does not block the test runner forever.
- Report the test results to the user and set a successful or failing exit status.

Such a test runner is useful to many projects, so we decided to add one to the `bootimage` tool.

### Bootimage Test

The test runner of the `bootimage` tool can be invoked via `bootimage test`. It uses the following conventions:

- All executables starting with `test-` are treated as integration tests.
- Tests must print either `ok` or `failed` over the serial port. When printing `failed` they can print additional information such as a panic message (in the next lines).
- Tests are run with a timeout of 1 minute. If the test has not completed in time, it is reported as "timed out".

The `test-basic-boot` and `test-panic` tests we created above begin with `test-` and follow the `ok`/`failed` conventions, so they should work with `bootimage test`:

```
> bootimage test
test-panic
    Finished dev [unoptimized + debuginfo] target(s) in 0.01s
Ok

test-basic-boot
    Finished dev [unoptimized + debuginfo] target(s) in 0.01s
Ok

test-something
    Finished dev [unoptimized + debuginfo] target(s) in 0.01s
Timed Out

The following tests failed:
    test-something: TimedOut
```

We see that our `test-panic` and `test-basic-boot` succeeded and that the `test-something` test timed out after one minute. We no longer need `test-something`, so we delete it (if you haven't done already). Now `bootimage test` should execute successfully.

## Summary

In this post we learned about the serial port and port-mapped I/O and saw how to configure QEMU to print serial output to the command line. We also learned a trick how to exit QEMU without needing to implement a proper shutdown.

We then split our crate into a library and binary part in order to create additional executables for integration tests. We added two example tests for testing that the `_start` function is correctly called and that a `panic` invokes our panic handler. Finally, we presented `bootimage test` as a basic test runner for our integration tests.

We now have a working integration test framework and can finally start to implement functionality in our kernel. We will continue to use the test framework over the next posts to test new components we add.

## What's next?
In the next post, we will explore _CPU exceptions_. These exceptions are thrown by the CPU when something illegal happens, such as a division by zero or an access to an unmapped memory page (a so-called “page fault”). Being able to catch and examine these exceptions is very important for debugging future errors. Exception handling is also very similar to the handling of hardware interrupts, which is required for keyboard support.
