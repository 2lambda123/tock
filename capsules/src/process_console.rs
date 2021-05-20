//! Implements a text console over the UART that allows
//! a terminal to inspect and control userspace processes.
//!
//! Protocol
//! --------
//!
//! This module provides a simple text-based console to inspect and control
//! which processes are running. The console has five commands:
//!  - 'help' prints the available commands and arguments
//!  - 'status' prints the current system status
//!  - 'list' lists the current processes with their IDs and running state
//!  - 'stop n' stops the process with name n
//!  - 'start n' starts the stopped process with name n
//!  - 'fault n' forces the process with name n into a fault state
//!  - 'panic' causes the kernel to run the panic handler
//!
//! ### `list` Command Fields:
//!
//! - `PID`: The identifier for the process. This can change if the process
//!   restarts.
//! - `Name`: The process name.
//! - `Quanta`: How many times this process has exceeded its alloted time
//!   quanta.
//! - `Syscalls`: The number of system calls the process has made to the kernel.
//! - `Dropped Upcalls`: How many callbacks were dropped for this process
//!   because the queue was full.
//! - `Restarts`: How many times this process has crashed and been restarted by
//!   the kernel.
//! - `State`: The state the process is in.
//! - `Grants`: The number of grants that have been initialized for the process
//!   out of the total number of grants defined by the kernel.
//!
//! Setup
//! -----
//!
//! You need a device that provides the `hil::uart::UART` trait. This code
//! connects a `ProcessConsole` directly up to USART0:
//!
//! ```rust
//! # use kernel::{capabilities, hil, static_init};
//! # use capsules::process_console::ProcessConsole;
//!
//! pub struct Capability;
//! unsafe impl capabilities::ProcessManagementCapability for Capability {}
//!
//! let pconsole = static_init!(
//!     ProcessConsole<usart::USART>,
//!     ProcessConsole::new(&usart::USART0,
//!                  115200,
//!                  &mut console::WRITE_BUF,
//!                  &mut console::READ_BUF,
//!                  &mut console::COMMAND_BUF,
//!                  kernel,
//!                  Capability));
//! hil::uart::UART::set_client(&usart::USART0, pconsole);
//!
//! pconsole.initialize();
//! pconsole.start();
//! ```
//!
//! Buffer use and output
//! ---------------------
//! `ProcessConsole` does not use its own write buffer for output:
//! it uses the debug!() buffer, so as not to repeat all of its buffering and
//! to maintain a correct ordering with debug!() calls. The write buffer of
//! `ProcessConsole` is used solely for echoing what someone types.
//!
//! Using ProcessConsole
//! --------------------
//!
//! With this capsule properly added to a board's `main.rs` and that kernel
//! loaded to the board, make sure there is a serial connection to the board.
//! Likely, this just means connecting a USB cable from a computer to the board.
//! Next, establish a serial console connection to the board. An easy way to do
//! this is to run:
//!
//! ```shell
//! $ tockloader listen
//! ```
//!
//! With that console open, you can issue commands. For example, to see all of
//! the processes on the board, use `list`:
//!
//! ```text
//! $ tockloader listen
//! Using "/dev/cu.usbserial-c098e513000c - Hail IoT Module - TockOS"
//!
//! Listening for serial output.
//! ProcessConsole::start
//! Starting process console
//! Initialization complete. Entering main loop
//! Hello World!
//! list
//! PID    Name    Quanta  Syscalls  Dropped Upcalls  Restarts    State  Grants
//! 00     blink        0       113                0         0  Yielded    1/12
//! 01     c_hello      0         8                0         0  Yielded    3/12
//! ```
//!
//! To get a general view of the system, use the status command:
//!
//! ```text
//! status
//! Total processes: 2
//! Active processes: 2
//! Timeslice expirations: 0
//! ```
//!
//! and you can control processes with the `start` and `stop` commands:
//!
//! ```text
//! stop blink
//! Process blink stopped
//! ```

use core::cell::Cell;
use core::cmp;
use core::fmt::{write, Result, Write};
use core::str;
use kernel::capabilities::ProcessManagementCapability;
use kernel::common::cells::{OptionalCell, TakeCell};
use kernel::AppId;

use kernel::debug;
use kernel::hil::uart;
use kernel::introspection::KernelInfo;
use kernel::ErrorCode;
use kernel::Kernel;

// Since writes are character echoes, we do not need more than 4 bytes:
// the longest write is 3 bytes for a backspace (backspace, space, backspace).
pub static mut WRITE_BUF: [u8; 500] = [0; 500];
pub static mut QUEUE_BUF: [u8; 300] = [0; 300];
pub static mut SIZE: usize = 0;
// Since reads are byte-by-byte, to properly echo what's typed,
// we can use a very small read buffer.
pub static mut READ_BUF: [u8; 4] = [0; 4];
// Commands can be up to 32 bytes long: since commands themselves are 4-5
// characters, limiting arguments to 25 bytes or so seems fine for now.
pub static mut COMMAND_BUF: [u8; 32] = [0; 32];

#[derive(PartialEq, Eq, Copy, Clone)]
enum WriterState {
    Empty,
    KernelStart,
    KernelBss,
    KernelInit,
    KernelStack,
    KernelRoData,
    KernelText,
    ProcessStart,
    ProcessGrant,
    ProcessHeap,
    ProcessHeapUnused,
    ProcessData,
    ProcessStack,
    ProcessStackUnused,
    ProcessFlash,
    ProcessProtected,
}

impl Default for WriterState {
    fn default() -> Self {
        WriterState::Empty
    }
}

pub struct ProcessConsole<'a, C: ProcessManagementCapability> {
    uart: &'a dyn uart::UartData<'a>,
    tx_in_progress: Cell<bool>,
    tx_buffer: TakeCell<'static, [u8]>,
    queue_buffer: TakeCell<'static, [u8]>,
    queue_size: Cell<usize>,
    writer_state: Cell<WriterState>,
    writer_process: Cell<Option<AppId>>,
    rx_in_progress: Cell<bool>,
    rx_buffer: TakeCell<'static, [u8]>,
    command_buffer: TakeCell<'static, [u8]>,
    command_index: Cell<usize>,
    drivers: OptionalCell<&'static str>,

    /// Flag to mark that the process console is active and has called receive
    /// from the underlying UART.
    running: Cell<bool>,

    /// Internal flag that the process console should parse the command it just
    /// received after finishing echoing the last newline character.
    execute: Cell<bool>,
    kernel: &'static Kernel,
    capability: C,
}

pub struct ConsoleWriter {
    buf: [u8; 500],
    size: usize,
}
impl ConsoleWriter {
    pub fn new() -> ConsoleWriter {
        ConsoleWriter {
            buf: [0; 500],
            size: 0,
        }
    }
    pub fn clear(&mut self) {
        self.size = 0;
    }
}
impl Write for ConsoleWriter {
    fn write_str(&mut self, s: &str) -> Result {
        let curr = (s).as_bytes().len();
        self.buf[self.size..self.size + curr].copy_from_slice(&(s).as_bytes()[..]);
        self.size += curr;
        Ok(())
    }
}

fn exceeded_check(size: usize, allocated: usize) -> &'static str {
    if size > allocated {
        " EXCEEDED!"
    } else {
        "          "
    }
}

//macro generates list of drivers used by platform
#[macro_export]
macro_rules! driver_debug {
    ($vis:vis struct $struct:ident {$( $field:ident:$type:ty ),*,}) => {
        /// Supported drivers by the platform
        $vis struct $struct { $($field: $type),*}

        static driver_debug_str : &'static str = concat!($("\t",stringify!($field),"\n"),*);
    };
}

impl<'a, C: ProcessManagementCapability> ProcessConsole<'a, C> {
    pub fn new(
        uart: &'a dyn uart::UartData<'a>,
        tx_buffer: &'static mut [u8],
        rx_buffer: &'static mut [u8],
        queue_buffer: &'static mut [u8],
        cmd_buffer: &'static mut [u8],
        kernel: &'static Kernel,
        capability: C,
    ) -> ProcessConsole<'a, C> {
        ProcessConsole {
            uart: uart,
            tx_in_progress: Cell::new(false),
            tx_buffer: TakeCell::new(tx_buffer),
            queue_buffer: TakeCell::new(queue_buffer),
            queue_size: Cell::new(0),
            writer_state: Cell::new(WriterState::Empty),
            writer_process: Cell::new(None),
            rx_in_progress: Cell::new(false),
            rx_buffer: TakeCell::new(rx_buffer),
            command_buffer: TakeCell::new(cmd_buffer),
            command_index: Cell::new(0),
            running: Cell::new(false),
            execute: Cell::new(false),
            kernel: kernel,
            capability: capability,
            drivers: OptionalCell::empty(),
        }
    }

    pub fn start(&self, driver_str: &'static str) -> ReturnCode {
        if self.running.get() == false {
            self.drivers.set(driver_str);
            self.rx_buffer.take().map(|buffer| {
                self.rx_in_progress.set(true);
                let _ = self.uart.receive_buffer(buffer, 1);
                self.running.set(true);
            });
            //Starts the process console while printing base information about
            //The kernel version and the drivers installed
            let mut console_writer = ConsoleWriter::new();
            let _ = write(
                &mut console_writer,
                format_args!(
                    "Kernel version: {}\r\n",
                    option_env!("TOCK_KERNEL_VERSION").unwrap_or("unknown")
                ),
            );
            self.write_bytes(&(console_writer.buf)[..console_writer.size]);
            console_writer.clear();
            self.write_bytes(b"Drivers:\n");
            let _ = write(&mut console_writer, format_args!("{}", driver_str));
            self.write_bytes(&(console_writer.buf)[..console_writer.size]);
            self.write_bytes(b"Welcome to the process console.\n");
            self.write_bytes(
                b"Valid commands are: help status list stop start fault process kernel\n",
            );
        }
        Ok(())
    }
    //simple state machine that identifies the next state
    fn next_state(&self, state: WriterState) -> WriterState {
        match state {
            WriterState::KernelStart => WriterState::KernelBss,
            WriterState::KernelBss => WriterState::KernelInit,
            WriterState::KernelInit => WriterState::KernelStack,
            WriterState::KernelStack => WriterState::KernelRoData,
            WriterState::KernelRoData => WriterState::KernelText,
            WriterState::KernelText => WriterState::Empty,
            WriterState::ProcessStart => WriterState::ProcessGrant,
            WriterState::ProcessGrant => WriterState::ProcessHeapUnused,
            WriterState::ProcessHeapUnused => WriterState::ProcessHeap,
            WriterState::ProcessHeap => WriterState::ProcessData,
            WriterState::ProcessData => WriterState::ProcessStack,
            WriterState::ProcessStack => WriterState::ProcessStackUnused,
            WriterState::ProcessStackUnused => WriterState::ProcessFlash,
            WriterState::ProcessFlash => WriterState::ProcessProtected,
            WriterState::ProcessProtected => WriterState::Empty,
            WriterState::Empty => WriterState::Empty,
        }
    }
    //defines the behavior at each state
    fn create_state_buffer(&self, state: WriterState, process_id: Option<AppId>) {
        match state {
            WriterState::KernelBss => {
                let mut console_writer = ConsoleWriter::new();
                let kernel_info = KernelInfo::new(self.kernel);
                let init_bottom: usize = kernel_info.get_kernel_init_end() as usize;
                //let bss_start: usize = kernel_info.get_kernel_bss_start() as usize;
                let bss_bottom: usize = kernel_info.get_kernel_bss_end() as usize;
                let bss_size = bss_bottom - init_bottom;
                let _ = write(
                    &mut console_writer,
                    format_args!(
                        "\r\n ╔═══════════╤══════════════════════════════╗\
                    \r\n ║  Address  │ Region Name    Used (bytes)  ║\
                    \r\n ╚{:#010X}═╪══════════════════════════════╝\
                    \r\n             │   Bss        {:6}",
                        bss_bottom, bss_size
                    ),
                );
                self.write_bytes(&(console_writer.buf)[..console_writer.size]);
            }
            WriterState::KernelInit => {
                let mut console_writer = ConsoleWriter::new();
                let kernel_info = KernelInfo::new(self.kernel);
                let init_bottom: usize = kernel_info.get_kernel_init_end() as usize;
                //let init_start: usize = kernel_info.get_kernel_init_start() as usize;
                let stack_bottom: usize = kernel_info.get_kernel_stack_end() as usize;
                let init_size = init_bottom - stack_bottom;

                let _ = write(
                    &mut console_writer,
                    format_args!(
                        "\
                    \r\n  {:#010X} ┼─────────────────────────────── S\
                    \r\n             │   Init       {:6}            R",
                        init_bottom, init_size
                    ),
                );
                self.write_bytes(&(console_writer.buf)[..console_writer.size]);
            }
            WriterState::KernelStack => {
                let mut console_writer = ConsoleWriter::new();
                let kernel_info = KernelInfo::new(self.kernel);
                let stack_start: usize = kernel_info.get_kernel_stack_start() as usize;
                let stack_bottom: usize = kernel_info.get_kernel_stack_end() as usize;

                let stack_size = stack_bottom - stack_start;

                let _ = write(
                    &mut console_writer,
                    format_args!(
                        "\
                    \r\n  {:#010X} ┼─────────────────────────────── A\
                    \r\n             │ ▼ Stack      {:6}            M\
                    \r\n  {:#010X} ┼───────────────────────────────",
                        stack_bottom, stack_size, stack_start
                    ),
                );
                self.write_bytes(&(console_writer.buf)[..console_writer.size]);
            }
            WriterState::KernelRoData => {
                let mut console_writer = ConsoleWriter::new();
                let kernel_info = KernelInfo::new(self.kernel);
                let rodata_start: usize = kernel_info.get_kernel_rodata_start() as usize;
                let rodata_bottom: usize = kernel_info.get_kernel_rodata_end() as usize;

                let rodata_size = rodata_bottom - rodata_start;

                let _ = write(
                    &mut console_writer,
                    format_args!(
                        "\
                        \r\n             .....\
                     \r\n  {:#010X} ┼─────────────────────────────── F\
                     \r\n             │   RoData     {:6}            L",
                        rodata_bottom, rodata_size
                    ),
                );
                self.write_bytes(&(console_writer.buf)[..console_writer.size]);
            }
            WriterState::KernelText => {
                let mut console_writer = ConsoleWriter::new();
                let kernel_info = KernelInfo::new(self.kernel);
                let text_start: usize = kernel_info.get_kernel_text_start() as usize;
                //let text_bottom: usize = kernel_info.get_kernel_text_end() as usize;
                let rodata_start: usize = kernel_info.get_kernel_rodata_start() as usize;

                let text_size = rodata_start - text_start;

                let _ = write(
                    &mut console_writer,
                    format_args!(
                        "\
                     \r\n  {:#010X} ┼─────────────────────────────── A\
                     \r\n             │   Text       {:6}            S\
                     \r\n  {:#010X} ┼─────────────────────────────── H\
                     \r\n",
                        rodata_start, text_size, text_start
                    ),
                );
                self.write_bytes(&(console_writer.buf)[..console_writer.size]);
            }
            WriterState::ProcessGrant => {
                if process_id.is_none() {
                } else {
                    self.kernel
                        .process_each_capability(&self.capability, |process| {
                            let proc_id = process.appid();
                            if proc_id == process_id.unwrap() {
                                let mut console_writer = ConsoleWriter::new();
                                // SRAM addresses
                                let sram_end = process.mem_end() as usize;
                                let sram_grant_start = process.kernel_memory_break() as usize;

                                // SRAM sizes
                                let sram_grant_size = sram_end - sram_grant_start;
                                let sram_grant_allocated = sram_end - sram_grant_start;

                                let _ = write(
                                    &mut console_writer,
                                    format_args!(
                                    "\r\n ╔═══════════╤══════════════════════════════════════════╗\
                                    \r\n ║  Address  │ Region Name    Used | Allocated (bytes)  ║\
                                    \r\n ╚{:#010X}═╪══════════════════════════════════════════╝\
                                    \r\n             │ ▼ Grant      {:6} | {:6}{}",
                                        sram_end,
                                        sram_grant_size,
                                        sram_grant_allocated,
                                        exceeded_check(sram_grant_size, sram_grant_allocated),
                                    ),
                                );
                                self.write_bytes(&(console_writer.buf)[..console_writer.size]);
                            }
                        });
                }
            }
            WriterState::ProcessHeapUnused => {
                if process_id.is_none() {
                } else {
                    self.kernel
                        .process_each_capability(&self.capability, |process| {
                            let proc_id = process.appid();
                            if proc_id == process_id.unwrap() {
                                let mut console_writer = ConsoleWriter::new();
                                // SRAM addresses
                                let sram_grant_start = process.kernel_memory_break() as usize;
                                let sram_heap_end = process.app_memory_break() as usize;

                                let _ = write(
                                    &mut console_writer,
                                    format_args!(
                                        "\
                                    \r\n  {:#010X} ┼───────────────────────────────────────────\
                                    \r\n             │ Unused\
                                    \r\n  {:#010X} ┼───────────────────────────────────────────",
                                        sram_grant_start, sram_heap_end,
                                    ),
                                );
                                self.write_bytes(&(console_writer.buf)[..console_writer.size]);
                            }
                        });
                }
            }
            WriterState::ProcessHeap => {
                if process_id.is_none() {
                } else {
                    self.kernel.process_each_capability(
                        &self.capability,
                        |process| {
                            let proc_id = process.appid();
                            if proc_id == process_id.unwrap() {
                                let mut console_writer = ConsoleWriter::new();
                                let sram_grant_start = process.kernel_memory_break() as usize;
                                let sram_heap_end = process.app_memory_break() as usize;
                                let sram_heap_start: Option<usize> = process.get_app_heap_start();

                                match sram_heap_start {
                                    Some(sram_heap_start) => {
                                        let sram_heap_size = sram_heap_end - sram_heap_start;
                                        let sram_heap_allocated = sram_grant_start - sram_heap_start;

                                        let _ = write(
                                            &mut console_writer,
                                            format_args!(
                                                "\
                                            \r\n             │ ▲ Heap       {:6} | {:6}{}     S\
                                            \r\n  {:#010X} ┼─────────────────────────────────────────── R",
                                                sram_heap_size,
                                                sram_heap_allocated,
                                                exceeded_check(sram_heap_size, sram_heap_allocated),
                                                sram_heap_start,
                                            ),
                                        );
                                    }
                                    None => {
                                        let _ = write(
                                            &mut console_writer,
                                            format_args!(
                                                "\
                                            \r\n             │ ▲ Heap            ? |      ?               S\
                                            \r\n  ?????????? ┼─────────────────────────────────────────── R",
                                            ),
                                        );
                                    }
                                }
                                self.write_bytes(&(console_writer.buf)[..console_writer.size]);

                            }
                        },
                    );
                }
            }
            WriterState::ProcessData => {
                if process_id.is_none() {
                } else {
                    self.kernel.process_each_capability(
                        &self.capability,
                        |process| {
                            let proc_id = process.appid();
                            if proc_id == process_id.unwrap() {
                                let mut console_writer = ConsoleWriter::new();

                                let sram_heap_start: Option<usize> = process.get_app_heap_start();
                                let sram_stack_start: Option<usize> = process.get_app_stack_start();

                                match (sram_heap_start, sram_stack_start) {
                                    (Some(sram_heap_start), Some(sram_stack_start)) => {
                                        let sram_data_size = sram_heap_start - sram_stack_start;
                                        let sram_data_allocated = sram_data_size as usize;

                                        let _ = write(
                                            &mut console_writer,
                                            format_args!(
                                                "\
                                            \r\n             │ Data         {:6} | {:6}               A",
                                                sram_data_size, sram_data_allocated,
                                            ),
                                        );
                                    }
                                    _ => {
                                        let _ = write(
                                            &mut console_writer,
                                            format_args!(
                                                "\
                                            \r\n             │ Data              ? |      ?               A",
                                            ),
                                        );
                                    }
                                }
                                self.write_bytes(&(console_writer.buf)[..console_writer.size]);

                            }
                        },
                    );
                }
            }
            WriterState::ProcessStack => {
                if process_id.is_none() {
                } else {
                    self.kernel.process_each_capability(
                        &self.capability,
                        |process| {
                            let proc_id = process.appid();
                            if proc_id == process_id.unwrap() {
                                let mut console_writer = ConsoleWriter::new();

                                let sram_stack_start: Option<usize> = process.get_app_stack_start();
                                let sram_stack_bottom: Option<usize> = process.get_app_stack_end();
                                let sram_start = process.mem_start() as usize;

                                match (sram_stack_start, sram_stack_bottom) {
                                    (Some(sram_stack_start), Some(sram_stack_bottom)) => {
                                        let sram_stack_size = sram_stack_start - sram_stack_bottom;
                                        let sram_stack_allocated = sram_stack_start - sram_start;

                                        let _ = write(
                                            &mut console_writer,
                                            format_args!(
                                                "\
                                            \r\n  {:#010X} ┼─────────────────────────────────────────── M\
                                            \r\n             │ ▼ Stack      {:6} | {:6}{}",
                                                sram_stack_start,
                                                sram_stack_size,
                                                sram_stack_allocated,
                                                exceeded_check(sram_stack_size, sram_stack_allocated),
                                            ),
                                        );
                                    }
                                    _ => {
                                        let _ = write(
                                            &mut console_writer,
                                            format_args!(
                                                "\
                                            \r\n  ?????????? ┼─────────────────────────────────────────── M\
                                            \r\n             │ ▼ Stack           ? |      ?",
                                            ),
                                        );
                                    }
                                }
                                self.write_bytes(&(console_writer.buf)[..console_writer.size]);

                            }
                        },
                    );
                }
            }

            WriterState::ProcessStackUnused => {
                if process_id.is_none() {
                } else {
                    self.kernel
                        .process_each_capability(&self.capability, |process| {
                            let proc_id = process.appid();
                            if proc_id == process_id.unwrap() {
                                let mut console_writer = ConsoleWriter::new();

                                let sram_stack_bottom: Option<usize> = process.get_app_stack_end();
                                let sram_start = process.mem_start() as usize;

                                let _ = write(
                                    &mut console_writer,
                                    format_args!(
                                        "\
                                    \r\n  {:#010X} ┼───────────────────────────────────────────\
                                    \r\n             │ Unused\
                                    \r\n  {:#010X} ┴───────────────────────────────────────────\
                                    \r\n             .....",
                                        sram_stack_bottom.unwrap_or(0),
                                        sram_start
                                    ),
                                );
                                self.write_bytes(&(console_writer.buf)[..console_writer.size]);
                            }
                        });
                }
            }
            WriterState::ProcessFlash => {
                if process_id.is_none() {
                } else {
                    self.kernel.process_each_capability(
                        &self.capability,
                        |process| {
                            let proc_id = process.appid();
                            if proc_id == process_id.unwrap() {
                                let mut console_writer = ConsoleWriter::new();

                                // Flash
                                let flash_end = process.flash_end() as usize;
                                let flash_app_start = process.flash_non_protected_start() as usize;
                                let flash_app_size = flash_end - flash_app_start;


                                let _ = write(
                                    &mut console_writer,
                                    format_args!(
                                        "\
                                        \r\n  {:#010X} ┬─────────────────────────────────────────── F\
                                        \r\n             │ App Flash    {:6}                        L",
                                        flash_end,
                                        flash_app_size,
                                    ),
                                );
                                self.write_bytes(&(console_writer.buf)[..console_writer.size]);

                            }
                        },
                    );
                }
            }
            WriterState::ProcessProtected => {
                if process_id.is_none() {
                } else {
                    self.kernel.process_each_capability(
                        &self.capability,
                        |process| {
                            let proc_id = process.appid();
                            if proc_id == process_id.unwrap() {
                                let mut console_writer = ConsoleWriter::new();

                                // Flash
                                let flash_start = process.flash_start() as usize;
                                let flash_protected_size = process.flash_protected() as usize;
                                let flash_app_start = process.flash_non_protected_start() as usize;


                                let _ = write(
                                    &mut console_writer,
                                    format_args!(
                                        "\
                                        \r\n  {:#010X} ┼─────────────────────────────────────────── A\
                                        \r\n             │ Protected    {:6}                        S\
                                        \r\n  {:#010X} ┴─────────────────────────────────────────── H\
                                        \r\n",
                                        flash_app_start,
                                        flash_protected_size,
                                        flash_start
                                    ),
                                );
                                self.write_bytes(&(console_writer.buf)[..console_writer.size]);

                            }
                        },
                    );
                }
            }
            _ => {}
        }
    }
    // Process the command in the command buffer and clear the buffer.
    fn read_command(&self) {
        self.command_buffer.map(|command| {
            let mut terminator = 0;
            let len = command.len();
            for i in 0..len {
                if command[i] == 0 {
                    terminator = i;
                    break;
                }
            }

            if terminator > 0 {
                let cmd_str = str::from_utf8(&command[0..terminator]);

                match cmd_str {
                    Ok(s) => {
                        let clean_str = s.trim();

                        if clean_str.starts_with("help") {

                            self.write_bytes(b"Welcome to the process console.\n");
                            self.write_bytes(b"Valid commands are: help status list stop start fault process kernel\n");

                        } else if clean_str.starts_with("start") {
                            let argument = clean_str.split_whitespace().nth(1);
                            argument.map(|name| {
                                self.kernel.process_each_capability(
                                    &self.capability,
                                    |proc| {
                                        let proc_name = proc.get_process_name();
                                        if proc_name == name {
                                            proc.resume();
                                            let mut console_writer = ConsoleWriter::new();
                                            let _ = write(&mut console_writer,format_args!("Process {} resumed.\n", name));

                                            self.write_bytes(&(console_writer.buf)[..console_writer.size]);
                                        }
                                    },
                                );
                            });
                        } else if clean_str.starts_with("stop") {
                            let argument = clean_str.split_whitespace().nth(1);
                            argument.map(|name| {
                                self.kernel.process_each_capability(
                                    &self.capability,
                                    |proc| {
                                        let proc_name = proc.get_process_name();
                                        if proc_name == name {
                                            proc.stop();
                                            let mut console_writer = ConsoleWriter::new();
                                            let _ = write(&mut console_writer,format_args!("Process {} stopped\n", proc_name));

                                            self.write_bytes(&(console_writer.buf)[..console_writer.size]);
                                        }
                                    },
                                );
                            });
                        } else if clean_str.starts_with("fault") {
                            let argument = clean_str.split_whitespace().nth(1);
                            argument.map(|name| {
                                self.kernel.process_each_capability(
                                    &self.capability,
                                    |proc| {
                                        let proc_name = proc.get_process_name();
                                        if proc_name == name {
                                            proc.set_fault_state();
                                            let mut console_writer = ConsoleWriter::new();
                                            let _ = write(&mut console_writer,format_args!("Process {} now faulted\n", proc_name));

                                            self.write_bytes(&(console_writer.buf)[..console_writer.size]);
                                        }
                                    },
                                );
                            });
                        } else if clean_str.starts_with("list") {
                            self.write_bytes(b" PID    Name                Quanta  Syscalls  Dropped Callbacks  Restarts    State  Grants\n");
                            self.kernel
                                .process_each_capability(&self.capability, |proc| {
                                    let info: KernelInfo = KernelInfo::new(self.kernel);

                                    let pname = proc.get_process_name();
                                    let appid = proc.processid();
                                    let (grants_used, grants_total) = info.number_app_grant_uses(appid, &self.capability);
                                    let mut console_writer = ConsoleWriter::new();
                                    let _ = write(&mut console_writer,format_args!(
                                        "  {:?}\t{:<20}{:6}{:10}{:19}{:10}  {:?}{:5}/{}\n",
                                        appid,
                                        pname,
                                        proc.debug_timeslice_expiration_count(),
                                        proc.debug_syscall_count(),
                                        proc.debug_dropped_upcall_count(),
                                        proc.get_restart_count(),
                                        proc.get_state(),
                                        grants_used,
                                        grants_total));

                                    self.write_bytes(&(console_writer.buf)[..console_writer.size]);
                                });
                        } else if clean_str.starts_with("status") {
                            let info: KernelInfo = KernelInfo::new(self.kernel);
                            let mut console_writer = ConsoleWriter::new();
                            let _ = write(&mut console_writer,format_args!(
                                "Total processes: {}\n",
                                info.number_loaded_processes(&self.capability)));
                            self.write_bytes(&(console_writer.buf)[..console_writer.size]);
                            console_writer.clear();
                            let _ = write(&mut console_writer,format_args!(
                                "Active processes: {}\n",
                                info.number_active_processes(&self.capability)));
                            self.write_bytes(&(console_writer.buf)[..console_writer.size]);
                            console_writer.clear();
                            let _ = write(&mut console_writer,format_args!(
                                "Timeslice expirations: {}\n",
                                info.timeslice_expirations(&self.capability)));
                            self.write_bytes(&(console_writer.buf)[..console_writer.size]);
                        } else if clean_str.starts_with("process"){
                            let argument = clean_str.split_whitespace().nth(1);
                            argument.map(|name| {
                                self.kernel.process_each_capability(
                                    &self.capability,
                                    |proc| {
                                        let proc_name = proc.get_process_name();
                                        if proc_name == name {
                                            //prints process memory by moving the writer to the start state
                                            self.write_state(WriterState::ProcessStart,Some(proc.appid()));
                                        }
                                    },
                                );
                            });
                        }else if clean_str.starts_with("kernel"){
                            let mut console_writer = ConsoleWriter::new();
                            let _ = write(&mut console_writer,format_args!(
                                "Kernel version: {}\r\n",
                                option_env!("TOCK_KERNEL_VERSION").unwrap_or("unknown")));
                            self.write_bytes(&(console_writer.buf)[..console_writer.size]);
                            console_writer.clear();
                            if self.drivers.is_some() {
                                self.drivers.map(|driver| {
                                    self.write_bytes(b"Drivers:\n");
                                    let _ = write(&mut console_writer,format_args!(
                                        "{}",
                                        driver));
                                    self.write_bytes(&(console_writer.buf)[..console_writer.size]);
                                    console_writer.clear();
                                });
                            };
                            //prints kernel memory by moving the writer to the start state
                            self.write_state(WriterState::KernelStart,None);

                        } else {
                            self.write_bytes(b"Valid commands are: help status list stop start fault process kernel\n");
                        }
                    }
                    Err(_e) => {
                        let mut console_writer = ConsoleWriter::new();
                        let _ = write(&mut console_writer,format_args!("Invalid command: {:?}", command));
                        self.write_bytes(&(console_writer.buf)[..console_writer.size]);
                    }
                }
            }
        });
        self.command_buffer.map(|command| {
            command[0] = 0;
        });
        self.command_index.set(0);
    }
    fn write_state(&self, state: WriterState, process: Option<AppId>) {
        if self.writer_state.get() == WriterState::Empty {
            self.writer_state.replace(state);
            self.writer_process.replace(process);
        }


        if !self.tx_in_progress.get() {
            self.writer_state
                .replace(self.next_state(self.writer_state.take()));
            self.create_state_buffer(self.writer_state.get(), self.writer_process.get());
        }
    }
    fn write_byte(&self, byte: u8) -> ReturnCode {
        if self.tx_in_progress.get() {
            self.queue_buffer.map(|buf| {
                buf[self.queue_size.get()] = byte;
                self.queue_size.set(self.queue_size.get() + 1);
            });
            ReturnCode::EBUSY
        } else {
            self.tx_in_progress.set(true);
            self.tx_buffer.take().map(|buffer| {
                buffer[0] = byte;
                let _ = self.uart.transmit_buffer(buffer, 1);
            });
            Ok(())
        }
    }

    fn write_bytes(&self, bytes: &[u8]) -> Result<(), ErrorCode> {
        if self.tx_in_progress.get() {

            self.queue_buffer.map(|buf| {
                let size = self.queue_size.get();
                let len = cmp::min(bytes.len(), buf.len() - size);
                (&mut buf[size..size + len]).copy_from_slice(&bytes[..len]);
                self.queue_size.set(size + len);
            });
            ReturnCode::EBUSY
        } else {
            self.tx_in_progress.set(true);
            self.tx_buffer.take().map(|buffer| {
                let len = cmp::min(bytes.len(), buffer.len());
                // Copy elements of `bytes` into `buffer`
                (&mut buffer[..len]).copy_from_slice(&bytes[..len]);
                let _ = self.uart.transmit_buffer(buffer, len);
            });
            Ok(())
        }
    }
}

impl<'a, C: ProcessManagementCapability> uart::TransmitClient for ProcessConsole<'a, C> {
    fn transmitted_buffer(
        &self,
        buffer: &'static mut [u8],
        _tx_len: usize,
        _rcode: Result<(), ErrorCode>,
    ) {
        self.tx_buffer.replace(buffer);
        self.tx_in_progress.set(false);
        //if in the middle of an active state, finish the state machine
        if self.writer_state.get() != WriterState::Empty
            && self.writer_state.get() != WriterState::KernelStart
            && self.writer_state.get() != WriterState::ProcessStart
        {
            self.write_state(WriterState::Empty, None);
        }
        //check the queue for data
        self.queue_buffer.map(|buf| {
            let len = self.queue_size.get();
            if len != 0 {
                self.write_bytes(&buf[..len]);
            }
            //self.uart.transmit_buffer(buf, len);
            self.queue_size.set(0);
        });
        //when queue is empty then we can start the state machine
        //for a new input
        if self.writer_state.get() != WriterState::Empty {
            self.write_state(WriterState::Empty, None);
        }

        // Check if we just received and echoed a newline character, and
        // therefore need to process the received message.
        if self.execute.get() {
            self.execute.set(false);
            self.read_command();
        }
    }
}
impl<'a, C: ProcessManagementCapability> uart::ReceiveClient for ProcessConsole<'a, C> {
    fn received_buffer(
        &self,
        read_buf: &'static mut [u8],
        rx_len: usize,
        _rcode: Result<(), ErrorCode>,
        error: uart::Error,
    ) {
        if error == uart::Error::None {
            match rx_len {
                0 => debug!("ProcessConsole had read of 0 bytes"),
                1 => {
                    self.command_buffer.map(|command| {
                        let index = self.command_index.get() as usize;
                        if read_buf[0] == ('\n' as u8) || read_buf[0] == ('\r' as u8) {
                            self.execute.set(true);
                            let _ = self.write_bytes(&['\r' as u8, '\n' as u8]);
                        } else if read_buf[0] == ('\x08' as u8) && index > 0 {
                            // Backspace, echo and remove last byte
                            // Note echo is '\b \b' to erase
                            let _ = self.write_bytes(&['\x08' as u8, ' ' as u8, '\x08' as u8]);
                            command[index - 1] = '\0' as u8;
                            self.command_index.set(index - 1);
                        } else if index < (command.len() - 1) && read_buf[0] < 128 {
                            // For some reason, sometimes reads return > 127 but no error,
                            // which causes utf-8 decoding failure, so check byte is < 128. -pal

                            // Echo the byte and store it
                            let _ = self.write_byte(read_buf[0]);
                            command[index] = read_buf[0];
                            self.command_index.set(index + 1);
                            command[index + 1] = 0;
                        }
                    });
                }
                _ => debug!(
                    "ProcessConsole issues reads of 1 byte, but receive_complete was length {}",
                    rx_len
                ),
            };
        }
        self.rx_in_progress.set(true);
        let _ = self.uart.receive_buffer(read_buf, 1);
    }
}
