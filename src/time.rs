use stm32h7::stm32h753::Peripherals;

// the time facility uses two timers:
// - one at 200000 MHz, for 5ns percision
// - one at 1000 Hz, for 1ms precision
//
// this gives us a maximum runtime of over a year before we overflow, but 5ns percision


pub fn time_init(devices: &mut Peripherals) {
    // eable and reset TIM2 in the RCC
    devices.RCC.apb1lenr.modify(|_, w| {
        w.tim2en().set_bit()
        .tim5en().set_bit()
    });
    devices.RCC.apb1lrstr.modify(|_, w| {
        w.tim2rst().set_bit()
        .tim5rst().set_bit()
    });
    devices.RCC.apb1lrstr.modify(|_, w| {
        w.tim2rst().clear_bit()
        .tim5rst().clear_bit()
    });

    devices.TIM2.psc.modify(|_, w| {
        // apb1 clock is 200MHz (half of the 400MHz cpu clock), giving us a maximum resolution of 5ns
        w.psc().variant(1)
    });

    devices.TIM2.psc.modify(|_, w| {
        // apb1 clock is 200MHz (half of the 400MHz cpu clock), giving us a maximum resolution of 5ns
        w.psc().variant(1)
    });

    // todo: set the timer period to 200000 (ticking over every 1ms), continuous, and output a signal to trigger TIM5

}

pub fn time_nanos(devices: &mut Peripherals) -> u64 {
    (devices.TIM2.cnt.read().cnt().bits() as u64 * 5).wrapping_add( 
        devices.TIM5.cnt.read().cnt().bits() as u64 * 1000000
    )
}

pub fn time_micros(devices: &mut Peripherals) -> u64 {
    (devices.TIM2.cnt.read().cnt().bits() as u64 / 200).wrapping_add(
        devices.TIM5.cnt.read().cnt().bits() as u64 * 1000
    )
}

pub fn time_millis(devices: &mut Peripherals) -> u64 {
    devices.TIM5.cnt.read().cnt().bits() as u64
}

pub fn time_seconds(devices: &mut Peripherals) -> u64 {
    devices.TIM5.cnt.read().cnt().bits() as u64 / 1000
}

// will retain full precision to 0.7 years
pub fn time_seconds_f64(devices: &mut Peripherals) -> f64 {
    time_nanos(devices) as f64 / 1000000000.0
}