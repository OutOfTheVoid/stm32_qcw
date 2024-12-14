#![no_main]
#![no_std]

extern crate panic_halt;
extern crate cortex_m_rt;
extern crate cortex_m;
extern crate stm32h7;

use core::u16;

use cortex_m_rt::entry;
use device_access::{set_devices, with_devices_mut};
use pll_setup::{setup_system_pll, switch_cpu_to_system_pll};
use stm32h7::stm32h753;
use time::{block_micros, block_millis};

mod pll_setup;
mod time;
mod device_access;
mod debug_led;
mod qcw;

#[entry]
fn main() -> ! {
    set_devices(stm32h753::Peripherals::take().unwrap());

    with_devices_mut(|devices, _| {
        setup_system_pll(devices, pll_setup::SystemPllSpeed::MHz400);
        switch_cpu_to_system_pll(devices);
    });

    debug_led::init();
    time::init();
    qcw::init();

    unsafe { cortex_m::interrupt::enable() };

    let mut feedback_values: [u16; 3] = [0; 3];

    let mut zero_angle = 0.05f32;

    loop {
        let STARTUP_TIME_US: u64 = 60;
        let TOTAL_TIME_US: u64 = 400;
        let STARTUP_PERIOD: u16 = 666;
        let PERIOD_OFFSET_MAX: u16 = 100;

        feedback_values.fill(0);
        let t0 = time::micros();
        with_devices_mut(|devices, _| qcw::configure_signal_path(devices, qcw::SignalPathConfig::OpenLoop { period_clocks: STARTUP_PERIOD, conduction_angle: 0.3 }));
        
        // spend some time in open loop mode to ring up the primary
        loop {
            let now = time::micros();
            if now - t0 >= STARTUP_TIME_US {
                break;
            }
        }

        // then try and lock the loop
        loop {
            let now = time::micros();
            if now - t0 >= TOTAL_TIME_US {
                with_devices_mut(|devices, _| {
                    qcw::configure_signal_path(devices, qcw::SignalPathConfig::Disabled);
                    debug_led::set_with_devices(devices, false);
                });
                break;
            }
            let closed_loop = with_devices_mut(|devices, _| {
                if let Some(value) = qcw::read_capture_timer(devices) {
                    for i in (1..feedback_values.len()).rev() {
                        feedback_values[i] = feedback_values[i - 1];
                    }
                    feedback_values[0] = value;
                    if feedback_variance_acceptable(PERIOD_OFFSET_MAX, STARTUP_PERIOD, &feedback_values[..]) {
                        debug_led::set_with_devices(devices, true);
                        let mut feedback_value_total = 0;
                        for v in feedback_values.iter() {
                            feedback_value_total += *v as u32;
                        }
                        feedback_value_total /= feedback_values.len() as u32;
                        qcw::configure_signal_path(devices, qcw::SignalPathConfig::ClosedLoop { period_clocks: feedback_value_total as u16, conduction_angle: 0.5, zero_angle, delay_comp: 0 });
                        return true
                    }
                }
                false
            });
            if closed_loop {
                break;
            }
        };

        // now we're in closed loop
        loop {
            let now = time::micros();
            if now - t0 >= TOTAL_TIME_US {
                with_devices_mut(|devices, _| {
                    qcw::configure_signal_path(devices, qcw::SignalPathConfig::Disabled);
                    debug_led::set_with_devices(devices, false);
                });
                break;
            }
            with_devices_mut(|devices, _| {
                if let Some(value) = qcw::read_capture_timer(devices) {
                    qcw::configure_signal_path(devices, qcw::SignalPathConfig::ClosedLoop { period_clocks: value, conduction_angle: 0.5, zero_angle, delay_comp: 0 });
                }
            });
        }
        with_devices_mut(|devices, _| qcw::configure_signal_path(devices, qcw::SignalPathConfig::Disabled));

        block_millis(100);
    }
}

fn feedback_variance_acceptable(allowed_deviation: u16, min_period: u16, feedback_values: &[u16]) -> bool {
    let mut min = u16::MAX;
    let mut max = u16::MIN;
    for v in feedback_values.iter() {
        min = min.min(*v);
        max = max.max(*v);
    }
    min > min_period && (max - min) < allowed_deviation
}