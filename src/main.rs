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
use cortex_m::{asm::{self, wfi}, interrupt::Mutex};
use core::{borrow::BorrowMut, mem::MaybeUninit};

mod qcw_controller;
mod pll_setup;
mod time;
mod device_access;
mod debug_led;

#[entry]
fn main() -> ! {
    set_devices(stm32h753::Peripherals::take().unwrap());

    with_devices_mut(|devices, _| {
        setup_system_pll(devices, pll_setup::SystemPllSpeed::MHz400);
        switch_cpu_to_system_pll(devices);
    });

    debug_led::init();
    debug_led::set(true);
    time::init();

    let qcw_config = qcw_controller::Config {
        delay_compensation: 10,
        startup_period: 500,
        allowed_period_deviation: 100,
    };
    qcw_controller::init(qcw_config);

    unsafe { cortex_m::interrupt::enable() };

    loop {
        if !qcw_controller::is_running() {
            let t0 = time::micros();
            while (time::micros() - t0) < 50 {}

            //_ = qcw_controller::clear_overcurrent();
            qcw_controller::start(qcw_controller::RunMode::TestClosedLoop { phase: 1.0, time_us: 1000 });
        }
        debug_led::set(((time::micros() / 1000000) & 1) != 0);
        qcw_controller::update();
    }
}