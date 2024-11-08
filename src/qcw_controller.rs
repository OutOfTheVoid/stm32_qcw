use cortex_m::interrupt::Mutex;
use core::cell::RefCell;
use stm32h7::stm32h753::{Peripherals, interrupt};

static QCW_CONFIG: Mutex<RefCell<Config>> = Mutex::new(RefCell::new(Config {
    phase_limit_low: 0.0,
    phase_limit_high: 1.0
}));

#[derive(Copy, Clone, Debug)]
pub struct Config {
    pub phase_limit_low: f32,
    pub phase_limit_high: f32,
}

/*
QCW Controller Signal Path
--------------------------

        Frequency Detector
        __________________
                                                   *----[HRTIM Timer B]--------------*
 (feedback ct signal)                              |                                 |
[GPIO D5] ------------> (HRTIM_EEV3) --------*---> | (capture 1)                     | 
- OT: AF                - rising edge        |     |                                 |
- AF: 2                                      *---> | (reset counter)                 |
                                                   |                                 |
                                                   *---------------------------------*

        ! Accessed by software querying the cpt1 bit of HRTIM_TIMB.bisr
        ! Capture interrupt is disabled in favor of polling 

        OCD Detector
        -------------

(OCD Signal (active low))
[GPIO A11]
- OT: INPUT

        ! Accessed by software reading the GPIOA data in register

        Phase Shift Signal Generator
        ----------------------------

*----[HRTIM Timer A]--------------*
|                                 |
|                                 |
|                         (out_1) | ----------------------> (GPIO C6)
|                                 |                          OT: AF
|                                 |                          AF: 1
*---------------------------------*

*----[HRTIM Timer C]--------------*
|                                 |
|                                 |
|                         (out_1) | ----------------------> (GPIO A9)
|                                 |                          OT: AF
|                                 |                          AF: 2
|                                 |
*---------------------------------*

        ! Software programs HRTIM Timers A and C such that they are
        ! programmed to the same frequency, with a phase offset
        ! between them, and swaps their phase offsets back and forth every few
        ! (even) number of cycles, allowing each half of the bridge to hard-switch
        ! half of the time, rather than one always soft-switching and the other hard switching.

*/



pub fn init(devices: &mut Peripherals, config: Config) {
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

    // enable and reset GPIOC
    devices.RCC.ahb4enr.modify(|_, w| {
        w.gpiocen().set_bit()
    });
    devices.RCC.ahb4rstr.write(|w| {
        w.gpiocrst().set_bit()
    });
    devices.RCC.ahb4rstr.write(|w| {
        w.gpiocrst().clear_bit()
    });

    // configure GPIO C6 to be hrtim output A1, push-pull
    devices.GPIOC.moder.modify(|_, w| {
        w.moder6().alternate()
    });
    devices.GPIOC.otyper.modify(|_, w| {
        w.ot6().push_pull()
    });
    devices.GPIOC.pupdr.modify(|_, w| {
        w.pupdr6().floating()
    });
    devices.GPIOC.afrl.modify(|_, w| {
        w.afr6().af1()
    });

    // configure timer a
    devices.HRTIM_TIMA.timacr.modify(|_, w| {
        w.updgat().variant(0b0000)
    });
    devices.HRTIM_TIMA.timacr.modify(|_, w| {
        w
            .updgat().variant(0b0000)
            .preen().clear_bit()
            .dacsync().variant(0b00)
            .mstu().variant(false)
            .tx_rstu().set_bit()
            .tx_repu().clear_bit()
            .delcmp2().variant(0)
            .delcmp4().variant(0)
            .syncrstx().clear_bit()
            .syncstrtx().clear_bit()
            .pshpll().clear_bit()
            .cont().set_bit()
            .ck_pscx().variant(0b101)
    });

    // set timer a to go high on cmp1, and low on per and cmp2
    devices.HRTIM_TIMA.seta1r.modify(|_, w| {
        w.cmp1().set_bit()
    });
    devices.HRTIM_TIMA.rsta1r.modify(|_, w| {
        w.per().set_bit()
        .cmp2().set_bit()
    });

    // enable and reset GPIOA
    devices.RCC.ahb4enr.modify(|_, w| {
        w.gpioaen().set_bit()
    });
    devices.RCC.ahb4rstr.write(|w| {
        w.gpioarst().set_bit()
    });
    devices.RCC.ahb4rstr.write(|w| {
        w.gpioarst().clear_bit()
    });

    // configure GPIO A9 to be hrtim output C1, push-pull
    devices.GPIOA.moder.modify(|_, w| {
        w.moder9().alternate()
    });
    devices.GPIOA.otyper.modify(|_, w| {
        w.ot9().push_pull()
    });
    devices.GPIOA.pupdr.modify(|_, w| {
        w.pupdr9().floating()
    });
    devices.GPIOA.afrh.modify(|_, w| {
        w.afr9().af2()
    });

    // configure timer c
    devices.HRTIM_TIMC.timccr.modify(|_, w| {
        w.updgat().variant(0b0000)
    });
    devices.HRTIM_TIMC.timccr.modify(|_, w| {
        w
            .updgat().variant(0b0000)
            .preen().clear_bit()
            .dacsync().variant(0b00)
            .mstu().variant(false)
            .tx_rstu().set_bit()
            .tx_repu().clear_bit()
            .delcmp2().variant(0)
            .delcmp4().variant(0)
            .syncrstx().clear_bit()
            .syncstrtx().clear_bit()
            .pshpll().clear_bit()
            .cont().set_bit()
            .ck_pscx().variant(0b101)
    });

    // set timer c to go low on cmp1, and high on per and cmp2
    devices.HRTIM_TIMC.setc1r.modify(|_, w| {
        w
        .per().set_bit()
        .cmp2().set_bit()
    });
    devices.HRTIM_TIMC.rstc1r.modify(|_, w| {
        w.cmp1().set_bit()
    });

    // enable and reset GPIOD
    devices.RCC.ahb4enr.modify(|_, w| {
        w.gpioden().set_bit()
    });
    devices.RCC.ahb4rstr.write(|w| {
        w.gpiodrst().set_bit()
    });
    devices.RCC.ahb4rstr.write(|w| {
        w.gpiodrst().clear_bit()
    });

    // configure GPIO D5 to be an input for HRTIM_EEV3
    devices.GPIOD.afrl.modify(|_, w| {
        w.afr5().af2()
    });
    devices.GPIOD.pupdr.modify(|_, w| {
        w.pupdr5().pull_down()
    });
    devices.GPIOD.moder.modify(|_, w| {
        w.moder5().alternate()
    });

    // configure external event 3 to be HRTIM_EEV3 (GPIO D5),
    // triggered by rising edge
    devices.HRTIM_COMMON.eecr1.modify(|_, w| {
        w
            .ee3src().variant(0)
            .ee3sns().variant(1)
    });

    // configure timer b to capture external event 3 period
    devices.HRTIM_TIMB.timbcr.modify(|_, w| {
        w.ck_pscx().variant(0b101)
        .cont().clear_bit()
        .retrig().set_bit()
    });
    devices.HRTIM_TIMB.rstbr.modify(|_, w| {
        w.extevnt3().set_bit()
    });
    devices.HRTIM_TIMB.cpt1bcr.modify(|_, w| {
        w.exev3cpt().set_bit()
    });
    devices.HRTIM_TIMB.perbr.modify(|_, w| {
        w.perx().variant(0xF000)
    });
    // enable capture 1 interrupts
    devices.HRTIM_TIMB.timbdier5.modify(|_, w| {
        w.cmp1ie().set_bit()
    });
    // clear the capture interrupt flag
    devices.HRTIM_TIMB.timbicr.write(|w| {
        w.cpt1c().set_bit()
    });

    cortex_m::interrupt::free(|cs| *QCW_CONFIG.borrow(cs).borrow_mut() = config);
}

struct HrtimChannelTimings {
    pub per: u16,
    pub cmp1: u16,
    pub cmp2: u16,
}

fn compute_hrtim_channel_timings(period: u16, phase: f32) -> HrtimChannelTimings {
    let period = period & !1;
    let half_period = period >> 1;
    let phase_offset = (half_period as f32 * phase) as u16;

    HrtimChannelTimings {
        per: period,
        cmp1: half_period - phase_offset,
        cmp2: period - phase_offset,
    }
}

pub fn start(devices: &mut Peripherals) {
    devices.HRTIM_COMMON.oenr.write(|w| {
        w
            .ta1oen().set_bit()
            .tc1oen().set_bit()
    });

    devices.HRTIM_MASTER.mcr.modify(|_, w| {
        w
        .tacen().set_bit()
        .tccen().set_bit()
        .tbcen().set_bit()
        .sync_src().variant(0b10)
        .sync_out().variant(0)
    });
}

pub fn set_period_phase(devices: &mut Peripherals, period: u16, phase: f32, flip_phases: bool) {
    let (phase_limit_low, phase_limit_high) = cortex_m::interrupt::free(|cs| {
        let config = QCW_CONFIG.borrow(cs).borrow();
        (config.phase_limit_low, config.phase_limit_high)
    });
    let phase = phase.clamp(phase_limit_low, phase_limit_high);
    let alpha_timings = compute_hrtim_channel_timings(period, 0.0);
    let beta_timings = compute_hrtim_channel_timings(period, 1.0 - phase);
    let (channel_a_timings, channel_c_timings) = match flip_phases {
        false => (alpha_timings, beta_timings ),
        true =>  (beta_timings,  alpha_timings),
    };

    devices.HRTIM_TIMA.perar.modify(|_, w| {
        w.perx().variant(channel_a_timings.per)
    });
    devices.HRTIM_TIMA.cmp1ar.modify(|_, w| {
        w.cmp1x().variant(channel_a_timings.cmp1)
    });
    devices.HRTIM_TIMA.cmp2ar.modify(|_, w| {
        w.cmp2x().variant(channel_a_timings.cmp2)
    });

    devices.HRTIM_TIMC.percr.modify(|_, w| {
        w.perx().variant(channel_c_timings.per)
    });
    devices.HRTIM_TIMC.cmp1cr.modify(|_, w| {
        w.cmp1x().variant(channel_c_timings.cmp1)
    });
    devices.HRTIM_TIMC.cmp2cr.modify(|_, w| {
        w.cmp2x().variant(channel_c_timings.cmp2)
    });
}

pub fn get_frequency_counter_capture(devices: &mut Peripherals) -> Option<u16> {
    if devices.HRTIM_TIMB.timbisr.read().cpt1().bit_is_set() {
        devices.HRTIM_TIMB.timbicr.write(|w| {
            w.cpt1c().set_bit()
        });
        Some(devices.HRTIM_TIMB.cpt1br.read().cpt1x().bits())
    } else {
        None
    }
}

#[interrupt]
fn HRTIM_TIMB() {
}
