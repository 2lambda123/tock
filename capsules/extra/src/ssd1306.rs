// Licensed under the Apache License, Version 2.0 or the MIT License.
// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright Tock Contributors 2023.

//! SSD1306/SSD1315 OLED Screen

use crate::bus::{self, Bus, BusWidth};
use core::cell::Cell;
use kernel::hil::gpio::Pin;
use kernel::hil::screen;
use kernel::hil::time::{self, Alarm, ConvertTicks};
use kernel::utilities::cells::{OptionalCell, TakeCell};
use kernel::ErrorCode;

pub const BUFFER_SIZE: usize = 24;

const WIDTH: usize = 128;
const HEIGHT: usize = 64;

// #[derive(PartialEq)]
// pub struct Command {
//     pub id: u8,
//     pub parameters: Option<&'static [u8]>,
//     pub delay: u8,
// }

// const NOP: Command = Command {
//     id: 0x00,
//     parameters: None,
//     delay: 0,
// };

// const SW_RESET: Command = Command {
//     id: 0x01,
//     parameters: None,
//     delay: 150, // 255?
// };

// const SLEEP_IN: Command = Command {
//     id: 0x10,
//     parameters: None,
//     delay: 10,
// };

// const SLEEP_OUT: Command = Command {
//     id: 0x11,
//     parameters: None,
//     delay: 255,
// };

// #[allow(dead_code)]
// const PARTIAL_ON: Command = Command {
//     id: 0x12,
//     parameters: None,
//     delay: 0,
// };

// const INVOFF: Command = Command {
//     id: 0x20,
//     parameters: None,
//     delay: 0,
// };

// const INVON: Command = Command {
//     id: 0x21,
//     parameters: None,
//     delay: 120,
// };

// const DISPLAY_OFF: Command = Command {
//     id: 0x28,
//     parameters: None,
//     delay: 100,
// };

// const DISPLAY_ON: Command = Command {
//     id: 0x29,
//     parameters: None,
//     delay: 100,
// };

// const WRITE_RAM: Command = Command {
//     id: 0x2C,
//     parameters: Some(&[]),
//     delay: 0,
// };

// #[allow(dead_code)]
// const READ_RAM: Command = Command {
//     id: 0x2E,
//     parameters: None,
//     delay: 0,
// };

// const CASET: Command = Command {
//     id: 0x2A,
//     parameters: Some(&[0x00, 0x00, 0x00, 0x00]),
//     delay: 0,
// };

// const RASET: Command = Command {
//     id: 0x2B,
//     parameters: Some(&[0x00, 0x00, 0x00, 0x00]),
//     delay: 0,
// };

// const NORON: Command = Command {
//     id: 0x13,
//     parameters: None,
//     delay: 10,
// };

// #[allow(dead_code)]
// const IDLE_OFF: Command = Command {
//     id: 0x38,
//     parameters: None,
//     delay: 20,
// };

// #[allow(dead_code)]
// const IDLE_ON: Command = Command {
//     id: 0x39,
//     parameters: None,
//     delay: 0,
// };

// const COLMOD: Command = Command {
//     id: 0x3A,
//     parameters: Some(&[0x05]),
//     delay: 0,
// };

// const MADCTL: Command = Command {
//     id: 0x36,
//     /// Default Parameters:
//     parameters: Some(&[0x00]),
//     delay: 0,
// };

// pub type CommandSequence = &'static [SendCommand];

// #[macro_export]
// macro_rules! default_parameters_sequence {
//     ($($cmd:expr),+) => {
//         [$(SendCommand::Default($cmd), )+]
//     }
// }

pub const SEQUENCE_BUFFER_SIZE: usize = 24;

enum Command {
    // Charge Pump Commands
    /// Charge Pump Setting.
    SetChargePump { enable: bool },

    // Fundamental Commands
    /// SetContrastControl. Double byte command to select 1 out of 256 contrast
    /// steps. Contrast increases as the value increases.
    SetContrast { contrast: u8 },
    /// Entire Display On.
    EntireDisplayOn { ignore_ram: bool },
    /// Set Normal Display.
    SetDisplayInvert { inverse: bool },
    /// Set Display Off.
    SetDisplayOnOff { on: bool },

    // Scrolling Commands
    /// Continuous Horizontal Scroll. Right or Left Horizontal Scroll.
    ContinuousHorizontalScroll {
        left: bool,
        page_start: u8,
        interval: u8,
        page_end: u8,
    },
    /// Continuous Vertical and Horizontal Scroll. Vertical and Right Horizontal
    /// Scroll.
    ContinuousVerticalHorizontalScroll {
        left: bool,
        page_start: u8,
        interval: u8,
        page_end: u8,
        vertical_offset: u8,
    },
    /// Deactivate Scroll. Stop scrolling that is configured by scroll commands.
    DeactivateScroll = 0x2e,
    /// Activate Scroll. Start scrolling that is configured by scroll commands.
    ActivateScroll = 0x2f,
    /// Set Vertical Scroll Area. Set number of rows in top fixed area. The number
    /// of rows in top fixed area is referenced to the top of the GDDRAM (i.e. row
    /// 0).
    SetVerticalScrollArea { rows_fixed: u8, rows_scroll: u8 },

    // Addressing Setting Commands
    /// Set Lower Column Start Address for Page Addressing Mode.
    ///
    /// Set the lower nibble of the column start address register for Page
    /// Addressing Mode using X[3:0] as data bits. The initial display line register
    /// is reset to 0000b after RESET.
    SetLowerColumnStartAddress { address: u8 },
    /// Set Higher Column Start Address for Page Addressing Mode.
    ///
    /// Set the higher nibble of the column start address register for Page
    /// Addressing Mode using X[3:0] as data bits. The initial display line register
    /// is reset to 0000b after RESET.
    SetHigherColumnStartAddress { address: u8 },
    /// Set Memory Addressing Mode.
    SetMemoryAddressingMode { mode: u8 },
    /// Set Column Address. Setup column start and end address.
    SetColumnAddress { column_start: u8, column_end: u8 },
    /// Set Page Address. Setup page start and end address.
    SetPageAddress { page_start: u8, page_end: u8 },
    /// Set Page Start Address for Page Addressing Mode. Set GDDRAM Page Start
    /// Address (PAGE0~PAGE7) for Page Addressing Mode using X[2:0].
    SetPageStartAddress { address: u8 },

    // Hardware Configuration Commands
    /// Set Display Start Line. Set display RAM display start line register from
    /// 0-63 using X[5:0].
    SetDisplayStartLine { line: u8 },
    /// Set Segment Remap.
    SetSegmentRemap { reverse: bool },
    /// Set Multiplex Ratio.
    SetMultiplexRatio { ratio: u8 },
    /// Set COM Output Scan Direction.
    SetComScanDirection { decrement: bool },
    /// Set Display Offset. Set vertical shift by COM from 0-63.
    SetDisplayOffset { vertical_shift: u8 } = 0xd3,
    /// Set COM Pins Hardware Configuration
    SetComPins { alternative: bool, enable_com: bool },

    // Timing & Driving Scheme Setting Commands.
    /// Set Display Clock Divide Ratio/Oscillator Frequency.
    SetDisplayClockDivide {
        divide_ratio: u8,
        oscillator_frequency: u8,
    },
    /// Set Pre-charge Period.
    SetPrechargePeriod { phase1: u8, phase2: u8 },
    /// Set VCOMH Deselect Level.
    SetVcomDeselect { level: u8 },
}

impl Command {
    fn encode(&self, buffer: &SubSliceMut) {
        let take = match self {
            Self::SetChargePump { enable } => {
                buffer[0] = 0x8D;
                buffer[1] = 0x10 | ((enable as u8) << 2);
                2
            }
            Self::SetContrast { contrast } => {
                buffer[0] = 0x81;
                buffer[1] = contrast;
                2
            }
            Self::EntireDisplayOn { ignore_ram } => {
                buffer[0] = 0xa4 | (ignore_ram as u8);
                1
            }
            Self::SetDisplayInvert { inverse } => {
                buffer[0] = 0xa6 | (inverse as u8);
                1
            }
            Self::SetDisplayOnOff { on } => {
                buffer[0] = 0xae | (on as u8);
                1
            }
            Self::ContinuousHorizontalScroll {
                left: bool,
                page_start: u8,
                interval: u8,
                page_end: u8,
            } => {
                buffer[0] = 0x26 | (left as u8);
                buffer[1] = 0;
                buffer[2] = page_start;
                buffer[3] = interval;
                buffer[4] = page_end;
                buffer[5] = 0;
                buffer[6] = 0xff;
                7
            }
            Self::ContinuousVerticalHorizontalScroll {
                left: bool,
                page_start: u8,
                interval: u8,
                page_end: u8,
                vertical_offset: u8,
            } => {
                buffer[0] = 0x29 | (left as u8);
                buffer[1] = 0;
                buffer[2] = page_start;
                buffer[3] = interval;
                buffer[4] = page_end;
                buffer[5] = vertical_offset;
                6
            }
            Self::DeactivateScroll => {
                buffer[0] = 0x2e;
                1
            }
            Self::ActivateScroll => {
                buffer[0] = 0x2f;
                1
            }
            Self::SetVerticalScrollArea {
                rows_fixed: u8,
                rows_scroll: u8,
            } => {
                buffer[0] = 0xa3;
                buffer[1] = rows_fixed;
                buffer[2] = rows_scroll;
                3
            }
            Self::SetLowerColumnStartAddress { address: u8 } => {
                buffer[0] = 0x00 | (address & 0xF);
                1
            }
            Self::SetHigherColumnStartAddress { address: u8 } => {
                buffer[0] = 0x10 | (address & 0xF);
                1
            }
            Self::SetMemoryAddressingMode { mode: u8 } => {
                buffer[0] = 0x20;
                buffer[1] = mode;
                2
            }
            Self::SetColumnAddress {
                column_start: u8,
                column_end: u8,
            } => {
                buffer[0] = 0x21;
                buffer[1] = column_start;
                buffer[2] = column_end;
                3
            }
            Self::SetPageAddress {
                page_start: u8,
                page_end: u8,
            } => {
                buffer[0] = 0x22;
                buffer[1] = page_start;
                buffer[2] = page_end;
                3
            }
            Self::SetPageStartAddress { address: u8 } => {
                buffer[0] = 0xb0 | (address & 0x7);
                1
            }
            Self::SetDisplayStartLine { line: u8 } => {
                buffer[0] = 0x40 | (line & 0x3F);
                1
            }
            Self::SetSegmentRemap { reverse: bool } => {
                buffer[0] = 0xa0 | (reverse as u8);
                1
            }
            Self::SetMultiplexRatio { ratio: u8 } => {
                buffer[0] = 0xa8;
                buffer[1] = ratio;
                2
            }
            Self::SetComScanDirection { decrement: bool } => {
                buffer[0] = 0xc0 | ((decrement as u8) << 3);
                1
            }
            Self::SetDisplayOffset { vertical_shift: u8 } => {
                buffer[0] = 0xd3;
                buffer[1] = vertical_shift;
                2
            }
            Self::SetComPins {
                alternative: bool,
                enable_com: bool,
            } => {
                buffer[0] = 0xda;
                buffer[1] = ((alternative as u8) << 4) | ((enable_com as u8) << 5);
                2
            }
            Self::SetDisplayClockDivide {
                divide_ratio: u8,
                oscillator_frequency: u8,
            } => {
                buffer[0] = 0xd5;
                buffer[1] = ((oscillator_frequency & 0xF) << 4) | (divide_ratio & 0xf);
                2
            }
            Self::SetPrechargePeriod {
                phase1: u8,
                phase2: u8,
            } => {
                buffer[0] = 0xd9;
                buffer[1] = ((phase2 & 0xF) << 4) | (phase1 & 0xf);
                2
            }
            Self::SetVcomDeselect { level: u8 } => {
                buffer[0] = 0xdb;
                buffer[1] = ((level & 0xF) << 4);
                2
            }
        };

        // Move the available region of the buffer to what is remaining after
        // this command was encoded.
        buffer.slice(take..);
    }
}

// #define SSD1306_SETLOWCOLUMN 0x00  ///< Not currently used
// #define SSD1306_SETHIGHCOLUMN 0x10 ///< Not currently used
// #define SSD1306_SETSTARTLINE 0x40  ///< See datasheet

// #define SSD1306_EXTERNALVCC 0x01  ///< External display voltage source
// #define SSD1306_SWITCHCAPVCC 0x02 ///< Gen. display voltage from 3.3V

// #define SSD1306_RIGHT_HORIZONTAL_SCROLL 0x26              ///< Init rt scroll
// #define SSD1306_LEFT_HORIZONTAL_SCROLL 0x27               ///< Init left scroll
// #define SSD1306_VERTICAL_AND_RIGHT_HORIZONTAL_SCROLL 0x29 ///< Init diag scroll
// #define SSD1306_VERTICAL_AND_LEFT_HORIZONTAL_SCROLL 0x2A  ///< Init diag scroll
// #define SSD1306_DEACTIVATE_SCROLL 0x2E                    ///< Stop scroll
// #define SSD1306_ACTIVATE_SCROLL 0x2F                      ///< Start scroll
// #define SSD1306_SET_VERTICAL_SCROLL_AREA 0xA3
// }

#[derive(Copy, Clone, PartialEq)]
enum Status {
    Idle,
    Init,
    Reset1,
    Reset2,
    Reset3,
    Reset4,
    SendCommand(usize, usize, usize),
    SendCommandSlice(usize),
    SendParametersSlice,
    Delay,
    Error(ErrorCode),
}
#[derive(Copy, Clone, PartialEq)]
pub enum SendCommand {
    Nop,
    Default(&'static Command),
    // first usize is the position in the buffer
    // second usize is the length in the buffer starting from the position
    Position(&'static Command, usize, usize),
    // first usize is the position in the buffer (4 bytes - repeat times, length bytes data)
    // second usize is the length in the buffer
    // third usize is the number of repeats
    Repeat(&'static Command, usize, usize, usize),
    // usize is length
    Slice(&'static Command, usize),
}

pub struct Ssd1306<'a, I: i2c::I2CDevice, P: Pin> {
    bus: &'a B,
    alarm: &'a A,
    dc: Option<&'a P>,
    reset: Option<&'a P>,
    status: Cell<Status>,
    width: Cell<usize>,
    height: Cell<usize>,

    client: OptionalCell<&'a dyn screen::ScreenClient>,
    setup_client: OptionalCell<&'a dyn screen::ScreenSetupClient>,
    setup_command: Cell<bool>,

    sequence_buffer: TakeCell<'static, [SendCommand]>,
    position_in_sequence: Cell<usize>,
    sequence_len: Cell<usize>,
    command: Cell<&'static Command>,
    buffer: TakeCell<'static, [u8]>,

    power_on: Cell<bool>,

    write_buffer: TakeCell<'static, [u8]>,

    current_rotation: Cell<ScreenRotation>,

    screen: &'static ST77XXScreen,
}

impl<'a, I: I2CDevice<'a>> SSD1306<'a, I> {
    pub fn new(i2c: &'a I, buffer: &'static mut [u8]) -> ST77XX<'a, A, B, P> {
        dc.map(|dc| dc.make_output());
        reset.map(|reset| reset.make_output());
        ST77XX {
            alarm: alarm,

            dc: dc,
            reset: reset,
            bus: bus,

            status: Cell::new(Status::Idle),
            width: Cell::new(screen.default_width),
            height: Cell::new(screen.default_height),

            client: OptionalCell::empty(),
            setup_client: OptionalCell::empty(),
            setup_command: Cell::new(false),

            sequence_buffer: TakeCell::new(sequence_buffer),
            sequence_len: Cell::new(0),
            position_in_sequence: Cell::new(0),
            command: Cell::new(&NOP),
            buffer: TakeCell::new(buffer),

            power_on: Cell::new(false),

            write_buffer: TakeCell::empty(),

            current_rotation: Cell::new(ScreenRotation::Normal),

            screen: screen,
        }
    }

    fn init_screen(&self) {
        let commands = [
            Command::SetDisplayOnOff { on: false },
            Command::SetDisplayClockDivide {
                divide_ratio: 1,
                oscillator_frequency: 0x8,
            },
            Command::SetMultiplexRatio { ratio: 63 },
            Command::SetDisplayOffset { vertical_shift: 0 },
            Command::SetDisplayStartLine { line: 0 },
            Command::SetChargePump {
                enable: self.enable_charge_pump,
            },
            Command::SetMemoryAddressingMode { mode: 0 }, //horizontal
            Command::SetSegmentRemap { reverse: true },
            Command::SetComScanDirection { decrement: true },
            Command::SetComPins {
                alternative: true,
                enable_com: false,
            },
            Command::SetContrast { contrast: 0xcf },
            Command::SetPrechargePeriod {
                phase1: 0x1,
                phase2: 0xf,
            },
            Command::SetVcomDeselect { level: 2 },
            Command::EntireDisplayOn { ignore_ram: false },
            Command::SetDisplayInvert { inverse: false },
            Command::DeactivateScroll,
            Command::SetDisplayOnOff { on: true },
        ];

        self.send_sequence(commands);
    }

    fn on_off(&self, on: bool) {
        let commands = [Command::SetDisplayOnOff { on: on }];
        self.send_sequence(commands);
    }

    fn brightness(&self, brightness: u16) {
        let commands = [Command::SetContrast {
            contrast: (brightness >> 8) as u8,
        }];
        self.send_sequence(commands);
    }

    fn invert(&self, invert: bool) {
        let commands = [Command::SetDisplayInvert { inverse: invert }];
        self.send_sequence(commands);
    }

    fn set_window(&self, x: usize, y: usize, width: usize, height: usize) {
        let commands = [
            Command::SetPageAddress {
                page_start: y,
                page_end: y + height,
            },
            Command::SetColumnAddress {
                column_start: x,
                column_end: x + width,
            },
        ];
        self.send_sequence(commands);
    }

    fn write(&self, data: SubSliceMut<'static, u8>) -> Result<(), ErrorCode> {
        self.sequence_buffer
            .map_or(Err(ErrorCode::NOMEM), |buffer| {
                let buf_slice = SubSliceMut::new(buffer);

                // Specify this is data.
                buf_slice[0] = 0x40; // Co = 0, D/C̅ = 1

                // Move the window of the subslice after the command byte header.
                buf_slice.slice(1..);

                for (i, b) in data.iter().enumerate() {
                    buf_slice[i + 1] = b;
                }

                let tx_len = buf_slice.len();

                self.i2c.send(buf_slice.take(), tx_len)
            })
    }

    fn send_sequence(&self, sequence: &[Command]) -> Result<(), ErrorCode> {
        if self.status.get() == Status::Idle {
            self.sequence_buffer
                .map_or(Err(ErrorCode::NOMEM), |buffer| {
                    let buf_slice = SubSliceMut::new(buffer);

                    // Specify this is a series of command bytes.
                    buffer[0] = 0; // Co = 0, D/C̅ = 0

                    // Move the window of the subslice after the command byte header.
                    buf_slice.slice(1..);

                    for cmd in sequence.iter() {
                        cmd.encode(buf_slice);
                    }

                    let tx_len = buf_slice.len();

                    self.i2c.send(buf_slice.take(), tx_len)
                })
        } else {
            Err(ErrorCode::BUSY)
        }
    }

    fn send_sequence_buffer(&self) -> Result<(), ErrorCode> {
        if self.status.get() == Status::Idle {
            self.position_in_sequence.set(0);
            // set status to delay so that do_next_op will send the next item in the sequence
            self.status.set(Status::Delay);
            self.do_next_op();
            Ok(())
        } else {
            Err(ErrorCode::BUSY)
        }
    }

    fn send_command_with_default_parameters(&self, cmd: &'static Command) {
        let mut len = 0;
        self.buffer.map_or_else(
            || panic!("st77xx: send parameters has no buffer"),
            |buffer| {
                // buffer[0] = cmd.id;
                if let Some(parameters) = cmd.parameters {
                    for parameter in parameters.iter() {
                        buffer[len] = *parameter;
                        len += 1;
                    }
                }
            },
        );
        self.send_command(cmd, 0, len, 1);
    }

    fn send_command(&self, cmd: &'static Command, position: usize, len: usize, repeat: usize) {
        self.command.set(cmd);
        self.status.set(Status::SendCommand(position, len, repeat));
        self.dc.map(|dc| dc.clear());
        let _ = self.bus.set_addr(BusWidth::Bits8, cmd.id as usize);
    }

    fn send_command_slice(&self, cmd: &'static Command, len: usize) {
        self.command.set(cmd);
        self.dc.map(|dc| dc.clear());
        self.status.set(Status::SendCommandSlice(len));
        let _ = self.bus.set_addr(BusWidth::Bits8, cmd.id as usize);
    }

    fn send_parameters(&self, position: usize, len: usize, repeat: usize) {
        self.status.set(Status::SendCommand(0, len, repeat - 1));
        if len > 0 {
            self.buffer.take().map_or_else(
                || panic!("st77xx: send parameters has no buffer"),
                |buffer| {
                    // shift parameters
                    if position > 0 {
                        for i in position..len + position {
                            buffer[i - position] = buffer[i];
                        }
                    }
                    self.dc.map(|dc| dc.set());
                    let _ = self.bus.write(BusWidth::Bits8, buffer, len);
                },
            );
        } else {
            self.do_next_op();
        }
    }

    fn send_parameters_slice(&self, len: usize) {
        self.write_buffer.take().map_or_else(
            || panic!("st77xx: no write buffer"),
            |buffer| {
                self.status.set(Status::SendParametersSlice);
                self.dc.map(|dc| dc.set());
                let _ = self.bus.write(BusWidth::Bits16BE, buffer, len / 2);
            },
        );
    }

    fn rotation(&self, rotation: ScreenRotation) -> Result<(), ErrorCode> {
        if self.status.get() == Status::Idle {
            let rotation_bits = match rotation {
                ScreenRotation::Normal => 0x00,
                ScreenRotation::Rotated90 => 0x60,
                ScreenRotation::Rotated180 => 0xC0,
                ScreenRotation::Rotated270 => 0xA0,
            };
            match rotation {
                ScreenRotation::Normal | ScreenRotation::Rotated180 => {
                    self.width.set(self.screen.default_width);
                    self.height.set(self.screen.default_height);
                }
                ScreenRotation::Rotated90 | ScreenRotation::Rotated270 => {
                    self.width.set(self.screen.default_height);
                    self.height.set(self.screen.default_width);
                }
            };
            self.buffer.map_or_else(
                || panic!("st77xx: set rotation has no buffer"),
                |buffer| {
                    buffer[0] =
                        rotation_bits | MADCTL.parameters.map_or(0, |parameters| parameters[0])
                },
            );
            self.setup_command.set(true);
            self.send_command(&MADCTL, 0, 1, 1);
            self.current_rotation.set(rotation);
            Ok(())
        } else {
            Err(ErrorCode::BUSY)
        }
    }

    fn display_on(&self) -> Result<(), ErrorCode> {
        if self.status.get() == Status::Idle {
            if !self.power_on.get() {
                Err(ErrorCode::OFF)
            } else {
                self.setup_command.set(false);
                self.send_command_with_default_parameters(&DISPLAY_ON);
                Ok(())
            }
        } else {
            Err(ErrorCode::BUSY)
        }
    }

    fn display_off(&self) -> Result<(), ErrorCode> {
        if self.status.get() == Status::Idle {
            if !self.power_on.get() {
                Err(ErrorCode::OFF)
            } else {
                self.setup_command.set(false);
                self.send_command_with_default_parameters(&DISPLAY_OFF);
                Ok(())
            }
        } else {
            Err(ErrorCode::BUSY)
        }
    }

    fn display_invert_on(&self) -> Result<(), ErrorCode> {
        if self.status.get() == Status::Idle {
            if !self.power_on.get() {
                Err(ErrorCode::OFF)
            } else {
                self.setup_command.set(false);
                let cmd = if self.screen.inverted {
                    &INVOFF
                } else {
                    &INVON
                };
                self.send_command_with_default_parameters(cmd);
                Ok(())
            }
        } else {
            Err(ErrorCode::BUSY)
        }
    }

    fn display_invert_off(&self) -> Result<(), ErrorCode> {
        if self.status.get() == Status::Idle {
            if !self.power_on.get() {
                Err(ErrorCode::OFF)
            } else {
                self.setup_command.set(false);
                let cmd = if self.screen.inverted {
                    &INVON
                } else {
                    &INVOFF
                };
                self.send_command_with_default_parameters(cmd);
                Ok(())
            }
        } else {
            Err(ErrorCode::BUSY)
        }
    }

    fn do_next_op(&self) {
        match self.status.get() {
            Status::Delay => {
                let position = self.position_in_sequence.get();

                self.position_in_sequence
                    .set(self.position_in_sequence.get() + 1);
                if position < self.sequence_len.get() {
                    self.sequence_buffer.map_or_else(
                        || panic!("st77xx: do next op has no sequence buffer"),
                        |sequence| {
                            match sequence[position] {
                                SendCommand::Nop => {
                                    self.do_next_op();
                                }
                                SendCommand::Default(cmd) => {
                                    self.send_command_with_default_parameters(cmd);
                                }
                                SendCommand::Position(cmd, position, len) => {
                                    self.send_command(cmd, position, len, 1);
                                }
                                SendCommand::Repeat(cmd, position, len, repeat) => {
                                    self.send_command(cmd, position, len, repeat);
                                }
                                SendCommand::Slice(cmd, len) => {
                                    self.send_command_slice(cmd, len);
                                }
                            };
                        },
                    );
                } else {
                    self.status.set(Status::Idle);
                    if !self.power_on.get() {
                        self.client.map(|client| {
                            self.power_on.set(true);

                            client.screen_is_ready();
                        });
                    } else {
                        if self.setup_command.get() {
                            self.setup_command.set(false);
                            self.setup_client.map(|setup_client| {
                                setup_client.command_complete(Ok(()));
                            });
                        } else {
                            self.client.map(|client| {
                                if self.write_buffer.is_some() {
                                    self.write_buffer.take().map(|buffer| {
                                        client.write_complete(buffer, Ok(()));
                                    });
                                } else {
                                    client.command_complete(Ok(()));
                                }
                            });
                        }
                    }
                }
            }
            Status::SendCommand(parameters_position, parameters_length, repeat) => {
                if repeat == 0 {
                    self.dc.map(|dc| dc.clear());
                    let mut delay = self.command.get().delay as u32;
                    if delay > 0 {
                        if delay == 255 {
                            delay = 500;
                        }
                        self.set_delay(delay, Status::Delay)
                    } else {
                        self.status.set(Status::Delay);
                        self.do_next_op();
                    }
                } else {
                    self.send_parameters(parameters_position, parameters_length, repeat);
                }
            }
            Status::SendCommandSlice(len) => {
                self.send_parameters_slice(len);
            }
            Status::SendParametersSlice => {
                self.dc.map(|dc| dc.clear());
                let mut delay = self.command.get().delay as u32;
                if delay > 0 {
                    if delay == 255 {
                        delay = 500;
                    }
                    self.set_delay(delay, Status::Delay)
                } else {
                    self.status.set(Status::Delay);
                    self.do_next_op();
                }
            }
            Status::Reset1 => {
                // self.send_command_with_default_parameters(&NOP);
                self.reset.map(|reset| reset.clear());
                self.set_delay(10, Status::Reset2);
            }
            Status::Reset2 => {
                self.reset.map(|reset| reset.set());
                self.set_delay(120, Status::Reset3);
            }
            Status::Reset3 => {
                self.reset.map(|reset| reset.clear());
                self.set_delay(120, Status::Reset4);
            }
            Status::Reset4 => {
                self.reset.map(|reset| reset.set());
                self.set_delay(120, Status::Init);
            }
            Status::Init => {
                self.status.set(Status::Idle);
                let _ = self.send_sequence(self.screen.init_sequence);
            }
            Status::Error(error) => {
                if self.setup_command.get() {
                    self.setup_command.set(false);
                    self.setup_client.map(|setup_client| {
                        setup_client.command_complete(Err(error));
                    });
                } else {
                    self.client.map(|client| {
                        if self.write_buffer.is_some() {
                            self.write_buffer.take().map(|buffer| {
                                client.write_complete(buffer, Err(error));
                            });
                        } else {
                            client.command_complete(Err(error));
                        }
                    });
                }
                self.status.set(Status::Idle);
            }
            _ => {
                panic!("ST77XX status Idle");
            }
        };
    }

    fn set_memory_frame(
        &self,
        position: usize,
        sx: usize,
        sy: usize,
        ex: usize,
        ey: usize,
    ) -> Result<(), ErrorCode> {
        if sx <= self.width.get()
            && sy <= self.height.get()
            && ex <= self.width.get()
            && ey <= self.height.get()
            && sx <= ex
            && sy <= ey
        {
            let (ox, oy) = (self.screen.offset)(self.current_rotation.get());
            if self.status.get() == Status::Idle {
                self.buffer.map_or_else(
                    || panic!("st77xx: set memory frame has no buffer"),
                    |buffer| {
                        // CASET
                        buffer[position] = (((sx + ox) >> 8) & 0xFF) as u8;
                        buffer[position + 1] = ((sx + ox) & 0xFF) as u8;
                        buffer[position + 2] = (((ex + ox) >> 8) & 0xFF) as u8;
                        buffer[position + 3] = ((ex + ox) & 0xFF) as u8;
                        // RASET
                        buffer[position + 4] = (((sy + oy) >> 8) & 0xFF) as u8;
                        buffer[position + 5] = ((sy + oy) & 0xFF) as u8;
                        buffer[position + 6] = (((ey + oy) >> 8) & 0xFF) as u8;
                        buffer[position + 7] = ((ey + oy) & 0xFF) as u8;
                    },
                );
                Ok(())
            } else {
                Err(ErrorCode::BUSY)
            }
        } else {
            Err(ErrorCode::INVAL)
        }
    }

    pub fn init(&self) -> Result<(), ErrorCode> {
        if self.status.get() == Status::Idle {
            self.status.set(Status::Reset1);
            self.do_next_op();
            Ok(())
        } else {
            Err(ErrorCode::BUSY)
        }
    }

    /// set_delay sets an alarm and saved the next state after that.
    ///
    /// As argument, there are:
    ///  - the duration of the alarm in ms
    ///  - the status of the program after the alarm fires
    ///
    /// Example:
    ///  self.set_delay(10, Status::Idle);
    fn set_delay(&self, timer: u32, next_status: Status) {
        self.status.set(next_status);
        let interval = self.alarm.ticks_from_ms(timer);
        self.alarm.set_alarm(self.alarm.now(), interval);
    }
}

impl<'a, I: I2CDevice<'a>> screen::ScreenSetup<'a> for Ssd1306<'a, I> {
    fn set_client(&self, client: &'a dyn ScreenSetupClient) {
        self.setup_client.set(client);
    }

    fn set_resolution(&self, resolution: (usize, usize)) -> Result<(), ErrorCode> {
        //
        Err(NOSUPPORT)
    }

    fn set_pixel_format(&self, depth: ScreenPixelFormat) -> Result<(), ErrorCode> {
        Err(NOSUPPORT)
    }

    fn set_rotation(&self, rotation: ScreenRotation) -> Result<(), ErrorCode> {
        // self.rotation(rotation)

        Err(NOSUPPORT)
    }

    fn get_num_supported_resolutions(&self) -> usize {
        1
    }

    fn get_supported_resolution(&self, index: usize) -> Option<(usize, usize)> {
        match index {
            0 => Some((WIDTH, HEIGHT)),
            _ => None,
        }
    }

    fn get_num_supported_pixel_formats(&self) -> usize {
        1
    }

    fn get_supported_pixel_format(&self, index: usize) -> Option<ScreenPixelFormat> {
        match index {
            0 => Some(screen::ScreenPixelFormat::Mono),
            _ => None,
        }
    }
}

impl<'a, I: I2CDevice<'a>> screen::Screen<'a> for Ssd1306<'a, I> {
    fn set_client(&self, client: &'a dyn screen::ScreenClient) {
        self.client.set(client);
    }

    fn get_resolution(&self) -> (usize, usize) {
        (WIDTH, HEIGHT)
    }

    fn get_pixel_format(&self) -> screen::ScreenPixelFormat {
        screen::ScreenPixelFormat::Mono
    }

    fn get_rotation(&self) -> screen::ScreenRotation {
        screen::ScreenRotation::Normal
    }

    fn set_write_frame(
        &self,
        x: usize,
        y: usize,
        width: usize,
        height: usize,
    ) -> Result<(), ErrorCode> {
        // need deferred call

        self.frame.set((x, y, width, height));
        Ok(())
    }

    fn write(&self, buffer: SubSliceMut<'static, u8>) -> Result<(), ErrorCode> {
        if self.status.get() == Status::Idle {
            self.setup_command.set(false);
            self.write_buffer.replace(buffer);
            let buffer_len = self.buffer.map_or_else(
                || panic!("st77xx: buffer is not available"),
                |buffer| buffer.len(),
            );
            if buffer_len > 0 {
                // set buffer
                self.sequence_buffer.map_or_else(
                    || panic!("st77xx: write no sequence buffer"),
                    |sequence| {
                        sequence[0] = SendCommand::Slice(&WRITE_RAM, len);
                        self.sequence_len.set(1);
                    },
                );
                let _ = self.send_sequence_buffer();
                Ok(())
            } else {
                Err(ErrorCode::NOMEM)
            }
        } else {
            Err(ErrorCode::BUSY)
        }
    }

    fn write_continue(&self, buffer: &'static mut [u8], len: usize) -> Result<(), ErrorCode> {
        if self.status.get() == Status::Idle {
            self.setup_command.set(false);
            self.write_buffer.replace(buffer);
            self.send_parameters_slice(len);
            Ok(())
        } else {
            Err(ErrorCode::BUSY)
        }
    }

    fn set_brightness(&self, _brightness: usize) -> Result<(), ErrorCode> {
        Ok(())
    }

    fn set_power(&self, enabled: bool) -> Result<(), ErrorCode> {
        if enabled {
            self.display_on()
        } else {
            self.display_off()
        }
    }

    fn set_invert(&self, enabled: bool) -> Result<(), ErrorCode> {
        if enabled {
            self.display_invert_on()
        } else {
            self.display_invert_off()
        }
    }
}

impl<'a, A: Alarm<'a>, B: Bus<'a>, P: Pin> time::AlarmClient for ST77XX<'a, A, B, P> {
    fn alarm(&self) {
        self.do_next_op();
    }
}

impl<'a, A: Alarm<'a>, B: Bus<'a>, P: Pin> bus::Client for ST77XX<'a, A, B, P> {
    fn command_complete(
        &self,
        buffer: Option<&'static mut [u8]>,
        _len: usize,
        status: Result<(), ErrorCode>,
    ) {
        if let Some(buffer) = buffer {
            if self.status.get() == Status::SendParametersSlice {
                self.write_buffer.replace(buffer);
            } else {
                self.buffer.replace(buffer);
            }
        }

        if let Err(error) = status {
            self.status.set(Status::Error(error));
        }

        self.do_next_op();
    }
}

/************ ST7735 **************/
#[allow(dead_code)]
const GAMSET: Command = Command {
    id: 0x26,
    /// Default parameters: Gama Set
    parameters: Some(&[0]),
    delay: 0,
};

const FRMCTR1: Command = Command {
    id: 0xB1,
    /// Default Parameters:
    parameters: Some(&[0x01, 0x2C, 0x2D]),
    delay: 0,
};

const FRMCTR2: Command = Command {
    id: 0xB2,
    /// Default Parameters:
    parameters: Some(&[0x01, 0x2C, 0x2D]),
    delay: 0,
};

const FRMCTR3: Command = Command {
    id: 0xB3,
    /// Default Parameters:
    parameters: Some(&[0x01, 0x2C, 0x2D, 0x01, 0x2C, 0x2D]),
    delay: 0,
};

const INVCTR: Command = Command {
    id: 0xB4,
    /// Default Parameters:
    parameters: Some(&[0x07]),
    delay: 0,
};

const PWCTR1: Command = Command {
    id: 0xC0,
    /// Default Parameters:
    parameters: Some(&[0xA2, 0x02, 0x84]),
    delay: 0,
};

const PWCTR2: Command = Command {
    id: 0xC1,
    /// Default Parameters:
    parameters: Some(&[0xC5]),
    delay: 0,
};

const PWCTR3: Command = Command {
    id: 0xC2,
    /// Default Parameters:
    parameters: Some(&[0x0A, 0x00]),
    delay: 0,
};

const PWCTR4: Command = Command {
    id: 0xC3,
    /// Default Parameters:
    parameters: Some(&[0x8A, 0x2A]),
    delay: 0,
};

const PWCTR5: Command = Command {
    id: 0xC4,
    /// Default Parameters:
    parameters: Some(&[0x8A, 0xEE]),
    delay: 0,
};

const VMCTR1: Command = Command {
    id: 0xC5,
    /// Default Parameters:
    parameters: Some(&[0x0E]),
    delay: 0,
};

const GMCTRP1: Command = Command {
    id: 0xE0,
    /// Default Parameters:
    parameters: Some(&[
        0x02, 0x1c, 0x07, 0x12, 0x37, 0x32, 0x29, 0x2d, 0x29, 0x25, 0x2B, 0x39, 0x00, 0x01, 0x03,
        0x10,
    ]),
    delay: 0,
};

const GMCTRN1: Command = Command {
    id: 0xE1,
    /// Default Parameters:
    parameters: Some(&[
        0x03, 0x1d, 0x07, 0x06, 0x2E, 0x2C, 0x29, 0x2D, 0x2E, 0x2E, 0x37, 0x3F, 0x00, 0x00, 0x02,
        0x10,
    ]),
    delay: 0,
};

const ST7735_INIT_SEQUENCE: [SendCommand; 20] = crate::default_parameters_sequence!(
    &SW_RESET, &SLEEP_OUT, &FRMCTR1, &FRMCTR2, &FRMCTR3, &INVCTR, &PWCTR1, &PWCTR2, &PWCTR3,
    &PWCTR4, &PWCTR5, &VMCTR1, &INVOFF, &MADCTL, &COLMOD, &CASET, &RASET, &GMCTRP1, &GMCTRN1,
    &NORON
);

/************ ST7789H2 **************/

const PV_GAMMA_CTRL: Command = Command {
    id: 0xE0,
    parameters: Some(&[
        0xD0, 0x08, 0x11, 0x08, 0x0C, 0x15, 0x39, 0x33, 0x50, 0x36, 0x13, 0x14, 0x29, 0x2D,
    ]),
    delay: 0,
};

const NV_GAMMA_CTRL: Command = Command {
    id: 0xE1,
    parameters: Some(&[
        0xD0, 0x08, 0x10, 0x08, 0x06, 0x06, 0x39, 0x44, 0x51, 0x0B, 0x16, 0x14, 0x2F, 0x31,
    ]),
    delay: 0,
};

const PORCH_CTRL: Command = Command {
    id: 0xB2,
    parameters: Some(&[0x0C, 0x0C, 0x00, 0x33, 0x33]),
    delay: 0,
};

const GATE_CTRL: Command = Command {
    id: 0xB7,
    parameters: Some(&[0x35]),
    delay: 0,
};

const LCM_CTRL: Command = Command {
    id: 0xC0,
    parameters: Some(&[0x2C]),
    delay: 0,
};

const VDV_VRH_EN: Command = Command {
    id: 0xC2,
    parameters: Some(&[0x01, 0xC3]),
    delay: 0,
};

const VDV_SET: Command = Command {
    id: 0xC4,
    parameters: Some(&[0x20]),
    delay: 0,
};

const FR_CTRL: Command = Command {
    id: 0xC6,
    parameters: Some(&[0x0F]),
    delay: 0,
};

const VCOM_SET: Command = Command {
    id: 0xBB,
    parameters: Some(&[0x1F]),
    delay: 0,
};

const POWER_CTRL: Command = Command {
    id: 0xD0,
    parameters: Some(&[0xA4, 0xA1]),
    delay: 0,
};

const TEARING_EFFECT: Command = Command {
    id: 0x35,
    parameters: Some(&[0x00]),
    delay: 0,
};

const ST7789H2_INIT_SEQUENCE: [SendCommand; 22] = crate::default_parameters_sequence!(
    &SLEEP_IN,
    &SW_RESET,
    &SLEEP_OUT,
    &NORON,
    &COLMOD,
    &INVON,
    &CASET,
    &RASET,
    &PORCH_CTRL,
    &GATE_CTRL,
    &VCOM_SET,
    &LCM_CTRL,
    &VDV_VRH_EN,
    &VDV_SET,
    &FR_CTRL,
    &POWER_CTRL,
    &PV_GAMMA_CTRL,
    &NV_GAMMA_CTRL,
    &MADCTL,
    &DISPLAY_ON,
    &SLEEP_OUT,
    &TEARING_EFFECT
);

/******** LS016B8UY *********/

const VSYNC_OUTPUT: Command = Command {
    id: 0x35,
    parameters: Some(&[0x00]),
    delay: 0,
};

const NORMAL_DISPLAY: Command = Command {
    id: 0x36,
    parameters: Some(&[0x83]),
    delay: 0,
};

const PANEL_SETTING1: Command = Command {
    id: 0xB0,
    parameters: Some(&[0x01, 0xFE]),
    delay: 0,
};

const PANEL_SETTING2: Command = Command {
    id: 0xB1,
    parameters: Some(&[0xDE, 0x21]),
    delay: 0,
};

const OSCILLATOR: Command = Command {
    id: 0xB3,
    parameters: Some(&[0x02]),
    delay: 0,
};

const PANEL_SETTING_LOCK: Command = Command {
    id: 0xB4,
    parameters: None,
    delay: 0,
};

const PANEL_V_PORCH: Command = Command {
    id: 0xB7,
    parameters: Some(&[0x05, 0x33]),
    delay: 0,
};

const PANEL_IDLE_V_PORCH: Command = Command {
    id: 0xB8,
    parameters: Some(&[0x05, 0x33]),
    delay: 0,
};

const GVDD: Command = Command {
    id: 0xC0,
    parameters: Some(&[0x53]),
    delay: 0,
};

const OPAMP: Command = Command {
    id: 0xC2,
    parameters: Some(&[0x03, 0x12]),
    delay: 0,
};

const RELOAD_MTP_VCOMH: Command = Command {
    id: 0xC5,
    parameters: Some(&[0x00, 0x45]),
    delay: 0,
};

const PANEL_TIMING1: Command = Command {
    id: 0xC8,
    parameters: Some(&[0x04, 0x03]),
    delay: 0,
};

const PANEL_TIMING2: Command = Command {
    id: 0xC9,
    parameters: Some(&[0x5E, 0x08]),
    delay: 0,
};

const PANEL_TIMING3: Command = Command {
    id: 0xCA,
    parameters: Some(&[0x0A, 0x0C, 0x02]),
    delay: 0,
};

const PANEL_TIMING4: Command = Command {
    id: 0xCC,
    parameters: Some(&[0x03, 0x04]),
    delay: 0,
};

const PANEL_POWER: Command = Command {
    id: 0xD0,
    parameters: Some(&[0x0C]),
    delay: 0,
};

const LS0168BUY_TEARING_EFFECT: Command = Command {
    id: 0xDD,
    parameters: Some(&[0x00]),
    delay: 0,
};

const LS016B8UY_INIT_SEQUENCE: [SendCommand; 23] = default_parameters_sequence!(
    &VSYNC_OUTPUT,
    &COLMOD,
    &PANEL_SETTING1,
    &PANEL_SETTING2,
    &PANEL_V_PORCH,
    &PANEL_IDLE_V_PORCH,
    &PANEL_TIMING1,
    &PANEL_TIMING2,
    &PANEL_TIMING3,
    &PANEL_TIMING4,
    &PANEL_POWER,
    &OSCILLATOR,
    &GVDD,
    &RELOAD_MTP_VCOMH,
    &OPAMP,
    &LS0168BUY_TEARING_EFFECT,
    &PANEL_SETTING_LOCK,
    &SLEEP_OUT,
    &NORMAL_DISPLAY,
    &CASET,
    &RASET,
    &DISPLAY_ON,
    &IDLE_OFF
);

pub struct ST77XXScreen {
    init_sequence: &'static [SendCommand],
    default_width: usize,
    default_height: usize,
    inverted: bool,

    /// This function allows the translation of the image
    /// as some screen implementations might have off screen
    /// pixels for some of the rotations
    offset: fn(rotation: ScreenRotation) -> (usize, usize),
}

pub const ST7735: ST77XXScreen = ST77XXScreen {
    init_sequence: &ST7735_INIT_SEQUENCE,
    default_width: 128,
    default_height: 160,
    inverted: false,
    offset: |_| (0, 0),
};

pub const ST7789H2: ST77XXScreen = ST77XXScreen {
    init_sequence: &ST7789H2_INIT_SEQUENCE,
    default_width: 240,
    default_height: 240,
    inverted: true,
    offset: |rotation| match rotation {
        ScreenRotation::Rotated180 => (0, 80),
        ScreenRotation::Rotated270 => (80, 0),
        _ => (0, 0),
    },
};

pub const LS016B8UY: ST77XXScreen = ST77XXScreen {
    init_sequence: &LS016B8UY_INIT_SEQUENCE,
    default_width: 240,
    default_height: 240,
    inverted: false,
    offset: |_| (0, 0),
};
