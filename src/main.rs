#![no_main]
#![no_std]

extern crate panic_halt;
extern crate cortex_m_rt;
extern crate cortex_m;
extern crate stm32h7;
extern crate libm;

use core::u16;
use libm::{asinf, powf};

use cortex_m_rt::entry;
use device_access::{set_devices, with_devices_mut};
use pll_setup::{setup_system_pll, switch_cpu_to_system_pll};
use stm32h7::stm32h753;
use time::block_millis;

mod pll_setup;
mod time;
mod device_access;
mod debug_led;
mod qcw;

const ZERO_ANGLE: f32 = 0.05f32;
const STARTUP_TIME_US: u64 = 40;
const TOTAL_TIME_US: u64 = 12000;
const STARTUP_PERIOD: u16 = 650;
const PERIOD_OFFSET_MAX: u16 = 100;
const STARTUP_CONDUCTION_ANGLE: f32 = 0.2;

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
    let mut t_closed_loop_start: u64 = 0;

    loop {

        feedback_values.fill(0);
        let t0 = time::micros();
        with_devices_mut(|devices, _| qcw::configure_signal_path(devices, qcw::SignalPathConfig::OpenLoop { period_clocks: STARTUP_PERIOD, conduction_angle: STARTUP_CONDUCTION_ANGLE }));
        
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
                        qcw::configure_signal_path(devices, qcw::SignalPathConfig::ClosedLoop { period_clocks: feedback_value_total as u16, conduction_angle: STARTUP_CONDUCTION_ANGLE, zero_angle: ZERO_ANGLE, delay_comp: 0 });
                        return true
                    }
                }
                false
            });
            if closed_loop {
                t_closed_loop_start = now;
                break;
            }
        };

        // now we're in closed loop
        loop {
            let now = time::micros();
            let duration_closed_loop = TOTAL_TIME_US - (t_closed_loop_start - t0);
            let t_closed_loop = now - t_closed_loop_start;
            let closed_loop_fraction = t_closed_loop as f32 / duration_closed_loop as f32;
            let conduction_angle = feedback_ramp(closed_loop_fraction) * (0.5 - STARTUP_CONDUCTION_ANGLE) + STARTUP_CONDUCTION_ANGLE;
            if now - t0 >= TOTAL_TIME_US {
                with_devices_mut(|devices, _| {
                    qcw::configure_signal_path(devices, qcw::SignalPathConfig::Disabled);
                    debug_led::set_with_devices(devices, false);
                });
                break;
            }
            with_devices_mut(|devices, _| {
                if let Some(value) = qcw::read_capture_timer(devices) {
                    qcw::configure_signal_path(devices, qcw::SignalPathConfig::ClosedLoop { period_clocks: value, conduction_angle, zero_angle: ZERO_ANGLE, delay_comp: 0 });
                }
            });
        }
        with_devices_mut(|devices, _| qcw::configure_signal_path(devices, qcw::SignalPathConfig::Disabled));

        block_millis(500);
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

fn feedback_ramp(t: f32) -> f32 {
    t
}
