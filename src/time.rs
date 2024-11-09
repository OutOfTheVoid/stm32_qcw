use stm32h7::stm32h753::Peripherals;

use crate::device_access::{with_devices, with_devices_mut};

// the time facility uses two timers:
// - one at 200000 MHz, for 5ns percision
// - one at 1000 Hz, for 1ms precision
//
// this gives us a maximum runtime of over a year before we overflow, but 5ns percision


pub fn init() {
    with_devices_mut(|devices, _| {
        // apb1 clock is 200MHz (half of the 400MHz cpu clock), giving us a maximum resolution of 5ns
        // we divide this by 20 to get a max resolution of 100ns, counting up one millisecond every 20000 ticks
        devices.TIM3.psc.write(|w| {
            w.psc().variant(19)
        });
        devices.TIM3.arr.write(|w| {
            w.arr().variant(10_000)
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
        (devices.TIM2.cnt.read().cnt().bits() as u64 / 10).wrapping_add(
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