#![no_main]
#![no_std]

extern crate panic_halt;
extern crate cortex_m_rt;
extern crate cortex_m;
extern crate stm32h7;

use cortex_m_rt::entry;
use device_access::{set_devices, with_devices_mut};
use pll_setup::{setup_system_pll, switch_cpu_to_system_pll};
use stm32h7::stm32h753::{self, crc::init};
use cortex_m::{asm::wfi, interrupt::Mutex};
use core::{borrow::BorrowMut, mem::MaybeUninit};

mod qcw_controller;
mod pll_setup;
mod time;
mod device_access;

#[entry]
fn main() -> ! {
    set_devices(stm32h753::Peripherals::take().unwrap());

    with_devices_mut(|devices, _| {
        setup_system_pll(devices, pll_setup::SystemPllSpeed::MHz400);
        switch_cpu_to_system_pll(devices);
    });

    let initial_period = (400_000_000 / 400_000) as u16;

    let qcw_config = qcw_controller::Config {
        phase_limit_high: 1.0,
        phase_limit_low: 0.3,
        allowed_period_deviation: initial_period / 4
    };
    qcw_controller::init(qcw_config);

    unsafe { cortex_m::interrupt::enable() };
    
    qcw_controller::start(initial_period, 0.5);
    
    let mut phase = 0.5;

    loop {
        wfi();
        while phase < 1.0 {
            cortex_m::asm::delay(1000000);
            phase += 0.0001;
            qcw_controller::set_phase(phase);
        }
        while phase > 0.5 {
            cortex_m::asm::delay(1000000);
            phase -= 0.0001;
            qcw_controller::set_phase(phase);
        }
    }
}