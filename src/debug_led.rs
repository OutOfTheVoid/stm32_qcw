use crate::device_access::with_devices_mut;

pub fn init() {
    with_devices_mut(|devices, _| {
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
    })
}

pub fn set(state: bool) {
    with_devices_mut(|devices, _| {
        devices.GPIOC.odr.modify(|_, w| {
            w.odr13().bit(!state)
        });
    })
}