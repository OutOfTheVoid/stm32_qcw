use core::{borrow::BorrowMut, cell::{Cell, RefCell}, mem::MaybeUninit};

use cortex_m::interrupt::{CriticalSection, Mutex};
use stm32h7::stm32h753;

static DEVICES: Mutex<RefCell<MaybeUninit<stm32h753::Peripherals>>> = Mutex::new(RefCell::new(MaybeUninit::uninit()));

pub fn with_devices_mut<F: Fn(&mut stm32h753::Peripherals, &CriticalSection)> (f: F) {
    unsafe {
        cortex_m::interrupt::free(|cs| {
            f(DEVICES.borrow(cs).borrow_mut().assume_init_mut(), cs);
        })
    }
}

pub fn with_devices<F: Fn(&stm32h753::Peripherals, &CriticalSection)> (f: F) {
    unsafe {
        cortex_m::interrupt::free(|cs| {
            f(DEVICES.borrow(cs).borrow().assume_init_ref(), cs);
        })
    }
}

pub fn set_devices(devices: stm32h753::Peripherals) {
    cortex_m::interrupt::free(|cs| {
        DEVICES.borrow(cs).borrow_mut().write(devices);
    });
}
