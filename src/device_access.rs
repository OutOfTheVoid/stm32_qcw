use core::{cell::RefCell, mem::MaybeUninit};

use cortex_m::{asm::nop, interrupt::{CriticalSection, Mutex}};
use stm32h7::stm32h753;

static DEVICES: Mutex<RefCell<MaybeUninit<stm32h753::Peripherals>>> = Mutex::new(RefCell::new(MaybeUninit::uninit()));

pub fn with_devices_mut<R, F: FnOnce(&mut stm32h753::Peripherals, &CriticalSection) -> R> (f: F) -> R {
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

    // enable and reset GPIOA, GPIOC, GPIOD, and SYSCFG
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

    // if we're not already in VOS1, let's get there
    if devices.PWR.d3cr.read().vos().bits() != 0b11 {
        // reset and set ldoen
        devices.PWR.cr3.modify(|_, w| {
            w.ldoen().clear_bit()
        });
        devices.PWR.cr3.modify(|_, w| {
            w.ldoen().set_bit()
        });

        // set core voltage scaling to VOS1
        devices.PWR.d3cr.modify(|_, w| {
            w.vos().variant(0b11)
        });

        for _ in 0..100 {
            nop();
        }

        // wait for vos to stabilize
        while devices.PWR.d3cr.read().vosrdy().bit_is_clear() {}
    }

    // enable SYSCFG clock so we can enable overdrive in the system config power control register
    devices.RCC.apb4enr.modify(|_, w| {
        w.syscfgen().set_bit()
    });
    
    // enable overdrive in the system config power control register,
    // which takes us to VOS0
    devices.SYSCFG.pwrcr.modify(|_, w| {
        w.oden().set_bit()
    });

    for _ in 0..100 {
        nop();
    }

    // wait for it to stabilize again
    while devices.PWR.d3cr.read().vosrdy().bit_is_clear() {}

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

    // enable and reset USART2
    devices.RCC.apb1lenr.modify(|_, w| w.usart2en().set_bit());
    devices.RCC.apb1lrstr.modify(|_, w| w.usart2rst().set_bit());
    devices.RCC.apb1lrstr.modify(|_, w| w.usart2rst().clear_bit());

    // enable and reset ADC1/ADC2
    devices.RCC.ahb1enr.modify(|_, w| w.adc12en().set_bit());
    devices.RCC.ahb1rstr.modify(|_, w| w.adc12rst().set_bit());
    devices.RCC.ahb1rstr.modify(|_, w| w.adc12rst().clear_bit());

    cortex_m::interrupt::free(|cs| {
        DEVICES.borrow(cs).borrow_mut().write(devices);
    });
}
