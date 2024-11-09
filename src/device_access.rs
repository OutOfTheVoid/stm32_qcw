use core::{borrow::BorrowMut, cell::{Cell, RefCell}, mem::MaybeUninit};

use cortex_m::interrupt::{CriticalSection, Mutex};
use stm32h7::stm32h753;

static DEVICES: Mutex<RefCell<MaybeUninit<stm32h753::Peripherals>>> = Mutex::new(RefCell::new(MaybeUninit::uninit()));

pub fn with_devices_mut<R, F: Fn(&mut stm32h753::Peripherals, &CriticalSection) -> R> (f: F) -> R {
    unsafe {
        cortex_m::interrupt::free(|cs| {
            f(DEVICES.borrow(cs).borrow_mut().assume_init_mut(), cs)
        })
    }
}

pub fn with_devices<R, F: Fn(&stm32h753::Peripherals, &CriticalSection) -> R> (f: F) -> R {
    unsafe {
        cortex_m::interrupt::free(|cs| {
            f(DEVICES.borrow(cs).borrow().assume_init_ref(), cs)
        })
    }
}

pub fn set_devices(devices: stm32h753::Peripherals) {
    // enable and reset HRTIM
    devices.RCC.apb2enr.modify(|_, w| {
        w.hrtimen().set_bit()
    });
    devices.RCC.apb2rstr.write(|w| {
        w.hrtimrst().set_bit()
    });
    devices.RCC.apb2rstr.write(|w| {
        w.hrtimrst().clear_bit()
    });

    // enable and reset GPIOA, GPIOC, GPIOD
    devices.RCC.ahb4enr.modify(|_, w| {
        w
            .gpioaen().set_bit()
            .gpiocen().set_bit()
            .gpioden().set_bit()
    });
    devices.RCC.ahb4rstr.write(|w| {
        w
            .gpioarst().set_bit()
            .gpiocrst().set_bit()
            .gpiodrst().set_bit()
    });
    devices.RCC.ahb4rstr.write(|w| {
        w
            .gpioarst().clear_bit()
            .gpiocrst().clear_bit()
            .gpiodrst().clear_bit()
    });

    // enable and reset TIM3, TIM5
    devices.RCC.apb1lenr.modify(|_, w| {
        w
            .tim3en().set_bit()
            .tim5en().set_bit()
    });
    devices.RCC.apb1lrstr.modify(|_, w| {
        w
            .tim3rst().set_bit()
            .tim5rst().set_bit()
    });
    devices.RCC.apb1lrstr.modify(|_, w| {
        w
            .tim3rst().clear_bit()
            .tim5rst().clear_bit()
    });

    cortex_m::interrupt::free(|cs| {
        DEVICES.borrow(cs).borrow_mut().write(devices);
    });
}
