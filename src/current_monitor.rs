#![allow(unused)]

use crate::{device_access::with_devices_mut, time::block_micros};

// current monitor pin PA6 -> ADC12_INP3

pub fn init() {
    with_devices_mut(|devices, _| {
        // PA6 - analog mode
        devices.GPIOA.pupdr.modify(|_, w| w.pupdr6().floating());
        devices.GPIOA.moder.modify(|_, w|  w.moder6().analog());
        // adc uses sys_ck which is 200 MHz (PLL1 / 2)
        // maximum frequency is 50 MHz, so divide this by 2, since it will additionally
        // be divided by 2 before reaching the adc block, giving us a division by 4 for 50 MHz
        devices.ADC12_COMMON.ccr.modify(|_, w| {
            w
                .ckmode().sync_div4()
        });
        // clear deep power down
        // turn on vref generation
        devices.ADC1.cr.modify(|_, w|  {
            w
                .boost().set_bit()
                .deeppwd().clear_bit()
        });

        devices.ADC1.cr.modify(|_, w|  {
            w
                .advregen().set_bit()
        });
    });
    // wait 20 us for the vreg to startup
    block_micros(20);
    with_devices_mut(|devices, _| {
        // setup calibration for linearity and single ended operation and start it
        devices.ADC1.cr.modify(|_, w| {
            w
                .adcaldif().clear_bit()
                .adcallin().set_bit()
                .adcal().set_bit()
        });
        // wait for calibration to complete
        while devices.ADC1.cr.read().adcal().bit_is_set() {}
        // clear the ready flag so we can check for ready after enable
        devices.ADC1.isr.modify(|_, w| w.adrdy().clear());
        // enable the adc
        devices.ADC1.cr.modify(|_, w| w.aden().set_bit());
        // wait for the adc to be ready
        while devices.ADC1.isr.read().adrdy().bit_is_clear() {}
        // setup continuous conversion from input p3
        // 12 bit resolution
        // store in data register
        devices.ADC1.cfgr.modify(|_, w| {
            w
                .cont().set_bit()
                .discen().clear_bit()
                .res().twelve_bit()
                .dmngt().dr()
                .ovrmod().overwrite()
        });
        // preselect channel 3 to enable conversion
        devices.ADC1.pcsel.modify(|_, w| unsafe { w.pcsel().bits(0b1000) });
        // select 1 conversion on channel 3
        devices.ADC1.sqr1.modify(|_, w| {
            w
                .l().variant(0)
                .sq1().variant(3)
        });
        // set sampling time to allow adc capacitor to charge to io voltage
        devices.ADC1.smpr1.modify(|_, w| w.smp3().cycles16_5());
        // start continuous conversion
        devices.ADC1.cr.modify(|_, w| w.adstart().set_bit());
    });

    //_ = get_raw();
}

pub fn get_raw() -> u16 {
    with_devices_mut(|devices, _| {
        while devices.ADC1.isr.read().eoc().bit_is_clear() {}
        devices.ADC1.dr.read().rdata().bits() & 0xFFF
    })
}

pub fn get_current() -> f32 {
    (get_raw() as f32 - 80.4) / 10.816
}

//   3A = ~904
// 2.5A = ~706
//   2A = ~640
// 1.5A = ~490
//   1A = ~360
// 