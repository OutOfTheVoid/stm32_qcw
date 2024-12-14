#![allow(unused)]

use stm32h7::stm32h753::Peripherals;

use crate::device_access::{with_devices, with_devices_mut};

pub fn init() {
    with_devices_mut(|devices, _| {
        devices.TIM3.psc.write(|w| {
            w.psc().variant(19) // prescale by 20, which gives us a 10MHz tim3
        });
        devices.TIM3.arr.write(|w| {
            w.arr().variant(10_000) // reload (period) of 10_000 clocks at 100ns each, which means a period of 1ms
        });
        devices.TIM3.cr2.modify(|_, w| {
            w.mms().update()
        });
        devices.TIM3.egr.write(|w| {
            w.ug().set_bit()
        });

        devices.TIM5.psc.write(|w| {
            w.psc().variant(0)
        });
        devices.TIM5.arr.write(|w| {
            w.arr().variant(0xFFFF_FFFF)
        });
        devices.TIM5.smcr.modify(|_, w| {
            w
            .sms().ext_clock_mode()
            // use timer3 as the trigger source
            .ts().itr2()
        });
        // start timers
        devices.TIM5.cr1.modify(|_, w| {
            w.cen().set_bit()
        });
        devices.TIM3.cr1.modify(|_, w| {
            w.cen().set_bit()
        });
    });
}

pub fn nanos() -> u64 {
    with_devices(|devices, _| {
        (devices.TIM3.cnt.read().cnt().bits() as u64 * 100).wrapping_add( 
            devices.TIM5.cnt.read().cnt().bits() as u64 * 1_000_000
        )
    })
}

pub fn micros() -> u64 {
    with_devices(|devices, _| {
        (devices.TIM3.cnt.read().cnt().bits() as u64 / 10).wrapping_add(
            devices.TIM5.cnt.read().cnt().bits() as u64 * 1000
        )
    })
}

pub fn millis() -> u64 {
    with_devices(|devices, _| {
        devices.TIM5.cnt.read().cnt().bits() as u64
    })
}

pub fn seconds() -> u64 {
    with_devices(|devices, _| {
        devices.TIM5.cnt.read().cnt().bits() as u64 / 1000
    })
}

// will retain full precision to 0.7 years
pub fn seconds_f64() -> f64 {
    nanos() as f64 / 1000000000.0
}

pub fn block_micros(n: u64) {
    let t0 = micros();
    while micros() - t0 < n {}
}

pub fn block_millis(n: u64) {
    let t0 = millis();
    while millis() - t0 < n {}
}
