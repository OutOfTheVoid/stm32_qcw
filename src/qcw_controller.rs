use cortex_m::{interrupt::Mutex, peripheral::nvic};
use core::cell::RefCell;
use stm32h7::stm32h753::{self, interrupt, Peripherals};

use crate::{device_access::{with_devices, with_devices_mut}, time};

#[derive(Copy, Clone, Debug)]
pub struct Config {
    pub delay_compensation: u16,
    pub startup_period: u16,
    pub allowed_period_deviation: u16
}

static QCW_CONFIG: Mutex<RefCell<Config>> = Mutex::new(RefCell::new(Config {
    delay_compensation: 0,
    allowed_period_deviation: 0,
    startup_period: 0,
}));

#[derive(Copy, Clone, Debug)]
pub enum RunMode {
    TestClosedLoop { phase: f32, time_us: u32 },
    TestOpenLoop { phase: f32, time_us: u32 },
    Burst { phase: f32, time_us: u32 },
    Ramp { phase_t0: f32, phase_t1: f32, time_us: u32 },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum OperationState {
    Idle,
    Locking,
    Running,
    RunningOpenLoop,
    Overcurrent,
}

struct QcwState {
    state: OperationState,
    run_mode: Option<RunMode>,
    t0: u64,
    period: u16,
    phase_setpoint: f32,
}

static QCW_STATE: Mutex<RefCell<QcwState>> = Mutex::new(RefCell::new(QcwState {
    state: OperationState::Idle,
    run_mode: None,
    t0: 0,
    period: 0,
    phase_setpoint: 0.0
}));

/*
QCW Controller Signal Path
--------------------------

        Frequency Detector
        __________________
                                                           *----[HRTIM Timer B]--------------*
        (feedback ct signal)                               |                                 |
        [GPIO D5] ------------> (HRTIM_EEV3) --------*---> | (capture 1)                     | 
        - OT: AF                - rising edge        |     |                                 |
        - AF: 2                                      *---> | (reset counter)                 |
                                                           |                                 |
                                                           *---------------------------------*

        ! Accessed by software querying the cpt1 bit of HRTIM_TIMB.bisr
        ! Capture interrupt is disabled in favor of polling 


        Feedback Delay
        --------------

    

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



pub fn init(config: Config) {
    with_devices_mut(|devices, cs| {
        // NOTE: peripheral clocks are enabled and peripherals reset in device_access.rs

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
                .preen().set_bit()
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
                .preen().set_bit()
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
            w.cpt1ie().set_bit()
        });
        // clear the capture interrupt flag
        devices.HRTIM_TIMB.timbicr.write(|w| {
            w.cpt1c().set_bit()
        });

        // configure timer d to delay external event 3
        devices.HRTIM_TIMD.timdcr.modify(|_, w| {
            w
                .tx_rstu().set_bit()
                .ck_pscx().variant(0b101)
                .cont().clear_bit()
                .retrig().set_bit()
                .preen().set_bit()
        });
        devices.HRTIM_TIMD.rstdr.modify(|_, w| {
            w.extevnt3().set_bit()
        });
        devices.HRTIM_TIMD.perdr.modify(|_, w| {
            w.perx().variant(0xF000)
        });
        devices.HRTIM_TIMD.cmp1dr.modify(|_, w| {
            w.cmp1x().variant(config.startup_period / 2 - config.delay_compensation)
        });

        // configure GPIO A11 as input (!OCD)
        devices.GPIOA.moder.modify(|_, w| {
            w.moder11().input()
        });
        devices.GPIOA.pupdr.modify(|_, w| {
            w.pupdr11().floating()
        });
        // setup gpio interrupt for a11
        devices.SYSCFG.exticr3.modify(|_, w| {
            w.exti11().variant(0)
        });
        devices.EXTI.ftsr1.modify(|_, w| {
            w.tr11().set_bit()
        });

        *QCW_CONFIG.borrow(cs).borrow_mut() = config;
        *QCW_STATE.borrow(cs).borrow_mut() = QcwState {
            state: OperationState::Idle,
            run_mode: None,
            t0: 0,
            period: 0,
            phase_setpoint: 0.0,
        }
    });
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

pub fn start(run_mode: RunMode) {
    let now = time::micros();
    let enable_feedback_interrupt = with_devices_mut(|devices, cs| {
        let config = {*QCW_CONFIG.borrow(cs).borrow()};

        let (operation_state, phase) = match &run_mode {
            RunMode::Burst { phase, .. } => (OperationState::Locking, *phase),
            RunMode::Ramp { phase_t0, .. } => (OperationState::Locking, *phase_t0),
            RunMode::TestClosedLoop { phase, ..  } => (OperationState::Running, *phase),
            RunMode::TestOpenLoop { phase, ..  } => (OperationState::RunningOpenLoop, *phase)
        };

        let period = config.startup_period;

        let mut state = QCW_STATE.borrow(cs).borrow_mut();
        if state.state == OperationState::Overcurrent {
            return false;
        }
        state.period = period;
        state.phase_setpoint = phase;
        state.run_mode = Some(run_mode);
        state.state = operation_state;
        state.t0 = now;

        begin_timer_update(devices);

        set_period_phase(devices, period, phase, false, config.delay_compensation);
        set_phase_timers_active(devices, true);

        let enable_feedback_interrupt = match operation_state {
            OperationState::Locking | OperationState::RunningOpenLoop => {
                set_feedback_triggering_active(devices, false);
                false
            },
            OperationState::Running => {
                set_feedback_triggering_active(devices, true);
                true
            },
            _ => false
        };

        end_timer_update(devices);

        
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
            .tdcen().set_bit()
            .sync_src().variant(0b10)
            .sync_out().variant(0)
        });
        enable_feedback_interrupt
    });
    set_feedback_interrupt_enabled(enable_feedback_interrupt);
}

fn set_feedback_interrupt_enabled(enabled: bool) {
    if enabled {
        unsafe { stm32h753::NVIC::unmask(interrupt::HRTIM_TIMB) };
    } else {
        stm32h753::NVIC::mask(interrupt::HRTIM_TIMB);
    }
}

fn set_phase_timers_active(devices: &mut Peripherals, active: bool) {
    if active {
        devices.HRTIM_TIMC.timccr.modify(|_, w| {
            w
                .cont().set_bit()
                .retrig().set_bit()
        });
        devices.HRTIM_TIMA.timacr.modify(|_, w| {
            w
                .cont().set_bit()
                .retrig().set_bit()
        });
    } else {
        devices.HRTIM_TIMC.timccr.modify(|_, w| {
            w
                .cont().clear_bit()
                .retrig().clear_bit()
        });
        devices.HRTIM_TIMA.timacr.modify(|_, w| {
            w
                .cont().clear_bit()
                .retrig().clear_bit()
        });
    }
}

fn set_feedback_triggering_active(devices: &mut Peripherals, active: bool) {
    if active {
        devices.HRTIM_TIMA.rstar.modify(|_, w| {
            w.timdcmp1().set_bit()
        });
        devices.HRTIM_TIMC.rstcr.modify(|_, w| {
            w.timdcmp1().set_bit()
        });
    } else {
        devices.HRTIM_TIMA.rstar.modify(|_, w| {
            w.timdcmp1().clear_bit()
        });
        devices.HRTIM_TIMC.rstcr.modify(|_, w| {
            w.timdcmp1().clear_bit()
        });
    }
}

pub fn update() {
    let now = time::micros();
    with_devices_mut(|devices, cs| {
        let config = *QCW_CONFIG.borrow(cs).borrow();
        let mut state = QCW_STATE.borrow(cs).borrow_mut();
        match (state.state, state.run_mode) {
            (OperationState::Idle, _) => {},
            (OperationState::Locking, _) => {
                if let Some(period) = read_frequency_detector() {
                    if (state.period as i32 - period as i32).abs() <= config.allowed_period_deviation as i32 {

                        state.period = period;
                        state.state = OperationState::Running;
                        
                        begin_timer_update(devices);
                        set_feedback_triggering_active(devices, true);
                        set_period_phase(devices, period, state.phase_setpoint, false, config.delay_compensation);
                        devices.HRTIM_TIMB.timbicr.write(|w| {
                            w.cpt1c().set_bit()
                        });
                        end_timer_update(devices);

                        set_feedback_interrupt_enabled(true);
                    }
                }
            },
            (_, Some(RunMode::TestOpenLoop { time_us, .. }) | Some(RunMode::TestClosedLoop { time_us, .. })) => {
                let time = now - state.t0;
                if time >= time_us as u64 {
                    begin_timer_update(devices);
                    set_phase_timers_active(devices, false);
                    end_timer_update(devices);
                    state.state = OperationState::Idle;
                    state.run_mode = None;
                    set_feedback_interrupt_enabled(false);
                }
            },
            (OperationState::Running, Some(RunMode::Burst { .. })) => {
                // todo - for now, immediately disable
                state.state = OperationState::Idle;
                state.run_mode = None;
                begin_timer_update(devices);
                set_phase_timers_active(devices, false);
                end_timer_update(devices);
                set_feedback_interrupt_enabled(false);
            }
            (OperationState::Running, Some(RunMode::Ramp { .. })) => {
                // todo - for now, immediately disable
                state.state = OperationState::Idle;
                state.run_mode = None;
                begin_timer_update(devices);
                set_phase_timers_active(devices, false);
                end_timer_update(devices);
                set_feedback_interrupt_enabled(false);
            }
            _ => {}
        }
    })
}

fn begin_timer_update(devices: &mut Peripherals) {
    devices.HRTIM_COMMON.cr1.modify(|_, w| {
        w
            .taudis().set_bit()
            .tcudis().set_bit()
            .tdudis().set_bit()
    });
}

fn end_timer_update(devices: &mut Peripherals) {
    devices.HRTIM_COMMON.cr1.modify(|_, w| {
        w
            .taudis().clear_bit()
            .tcudis().clear_bit()
            .tdudis().clear_bit()
    });
}

fn set_period_phase(devices: &mut Peripherals, period: u16, phase: f32, flip_phases: bool, delay_compensation: u16) {
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

    devices.HRTIM_TIMD.cmp1dr.modify(|_, w| {
        w.cmp1x().variant(period / 2 - delay_compensation)
    });
}

pub fn is_running() -> bool {
    with_devices(|_, cs| {
        let state = QCW_STATE.borrow(cs).borrow();
        match state.state {
           OperationState::Idle | OperationState::Overcurrent => false,
           _ => true
        }
    })
}

pub fn overcurrent_status() -> bool {
    with_devices(|devices, _| {
        devices.GPIOA.idr.read().idr11().bit_is_clear()
    })
}

pub fn clear_overcurrent() -> Result<(), ()> {
    with_devices(|devices, cs| {
        if devices.GPIOA.idr.read().idr11().bit_is_clear() {
            Err(())
        } else {
            let mut state = QCW_STATE.borrow(cs).borrow_mut();
            match state.state {
                OperationState::Idle | OperationState::Overcurrent => {
                    state.state = OperationState::Idle;
                    Ok(())
                },
                _ => Err(())
            }
        }
    })
}

fn read_frequency_detector() -> Option<u16> {
    with_devices_mut(|devices, _| {
        if devices.HRTIM_TIMB.timbisr.read().cpt1().bit_is_set() {
            let period = devices.HRTIM_TIMB.cpt1br.read().cpt1x().bits();
            devices.HRTIM_TIMB.timbicr.write(|w| {
                w.cpt1c().set_bit()
            });
            Some(period)
        } else {
            None
        }
    })
}

#[interrupt]
fn HRTIM_TIMB() {
    with_devices_mut(|devices, cs| {
        let config = {*QCW_CONFIG.borrow(cs).borrow()};
        if let Some(period) = read_frequency_detector() {
            let mut state = QCW_STATE.borrow(cs).borrow_mut();
            begin_timer_update(devices);
            set_period_phase(devices, period, state.phase_setpoint, false, config.delay_compensation);
            end_timer_update(devices);
            state.period = period;
        }
    });
}

#[interrupt]
fn EXTI15_10() {
    with_devices_mut(|devices, _| {
        if devices.EXTI.cpupr1.read().pr11().bit_is_set() {
            // OCD trigger
            devices.EXTI.cpupr1.modify(|_, w| {
                w.pr11().set_bit()
            });
        }
    });
}
