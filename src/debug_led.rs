#![allow(unused)]

use stm32h7::stm32h753::Peripherals;

use crate::device_access::with_devices_mut;

pub fn init() {
    with_devices_mut(|devices, _| {
        init_with_devices(devices)
    })
}

pub fn init_with_devices(devices: &mut Peripherals) {
    devices.GPIOC.moder.modify(|_, w| {
        w.moder13().output()
    });
    devices.GPIOC.otyper.modify(|_, w| {
        w.ot13().open_drain()
    });
    devices.GPIOC.pupdr.modify(|_, w| {
        w.pupdr13().pull_up()
    });
    devices.GPIOC.odr.write(|w| {
        w.odr13().set_bit()
    });
}

pub fn set(state: bool) {
    with_devices_mut(|devices, _| {
        set_with_devices(devices, state);
    })
}

pub fn set_with_devices(devices: &mut Peripherals, state: bool) {
    devices.GPIOC.odr.modify(|_, w| {
        w.odr13().bit(!state)
    });
}