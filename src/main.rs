#![no_main]
#![no_std]

extern crate panic_halt;
extern crate cortex_m_rt;
extern crate cortex_m;
extern crate stm32h7;

use cortex_m_rt::entry;
use pll_setup::{setup_system_pll, switch_cpu_to_system_pll};
use stm32h7::stm32h753;

mod qcw_controller;
mod pll_setup;
mod time;

#[entry]
fn main() -> ! {
    let mut devices = stm32h753::Peripherals::take().unwrap();

    setup_system_pll(&devices, pll_setup::SystemPllSpeed::MHz400);
    switch_cpu_to_system_pll(&devices);

    let qcw_config = qcw_controller::Config {
        phase_limit_high: 1.0,
        phase_limit_low: 0.3
    };
    qcw_controller::init(&mut devices, qcw_config);

    let mut period = 500;
    let mut x = 0.3;

    qcw_controller::set_period_phase(&mut devices, period, x, false);
    qcw_controller::start(&mut devices);

    loop {
        while x < 1.0 {
            cortex_m::asm::delay(1000000);
            x += 0.001;
            if let Some(counted_period) = qcw_controller::get_frequency_counter_capture(&mut devices) {
                period = counted_period;
            }
            qcw_controller::set_period_phase(&mut devices, period, x, false);
        }
        while x > 0.3 {
            cortex_m::asm::delay(1000000);
            x -= 0.001;
            if let Some(counted_period) = qcw_controller::get_frequency_counter_capture(&mut devices) {
                period = counted_period;
            }
            qcw_controller::set_period_phase(&mut devices, period, x, false);
        }
    }
}