use gabix_kernel::Platform;

#[cfg(feature = "virt")]
pub const UART_BASE: usize = 0x0900_0000;

#[cfg(feature = "virt")]
pub const PLATFORM: Platform = Platform::ArmVirt;

#[cfg(feature = "raspi2b")]
pub const UART_BASE: usize = 0x3f20_1000;

#[cfg(feature = "raspi2b")]
pub const PLATFORM: Platform = Platform::RaspberryPi2B;
