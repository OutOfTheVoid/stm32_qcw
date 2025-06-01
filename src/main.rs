#![no_main]
#![no_std]

extern crate panic_halt;
extern crate cortex_m_rt;
extern crate cortex_m;
extern crate stm32h7;
extern crate libm;

use alloc::collections::vec_deque::VecDeque;
use cortex_m_rt::entry;
use device_access::{set_devices, with_devices_mut};
use pll_setup::{setup_system_pll, switch_cpu_to_system_pll};
use qcw::SignalPathConfig;
use crate::serial_link::{SerialLink, SerialMailbox};
use qcw_com::*;
use stm32h7::stm32h753;

mod pll_setup;
mod time;
mod device_access;
mod debug_led;
mod qcw;
mod serial_link;
mod current_monitor;

extern crate alloc;
use embedded_alloc::LlffHeap as Heap;

#[global_allocator]
static HEAP: Heap = Heap::empty();

pub struct QcwParameters {
    pub delay_compensation_ns: i16,
    pub startup_frequency_khz: f32,
    pub lock_range_khz: f32,

    pub run_mode: RunMode,

    pub ontime_us: u64,
    pub offtime_ms: u64,

    pub startup_time_us: u64,
    pub lock_time_us: u64,
    pub min_lock_current: f32,

    pub current_limit: f32,

    pub ramp_start_power: f32,
    pub ramp_end_power: f32,
    pub flat_power: f32,
}

pub struct QcwStats {
    pub max_primary_current: f32,
    pub feedback_frequency_khz: f32,
}

const REMOTE_TIMEOUT_US: u64 = 100_000;

#[entry]
fn main() -> ! {
    {
        use core::mem::MaybeUninit;
        const HEAP_SIZE: usize = 8192;
        static mut HEAP_MEM: [MaybeUninit<u8>; HEAP_SIZE] = [MaybeUninit::uninit(); HEAP_SIZE];
        unsafe { HEAP.init(&raw mut HEAP_MEM as usize, HEAP_SIZE) }
    }

    set_devices(stm32h753::Peripherals::take().unwrap());

    with_devices_mut(|devices, _| {
        setup_system_pll(devices, pll_setup::SystemPllSpeed::MHz400);
        switch_cpu_to_system_pll(devices);
    });

    debug_led::init();
    time::init();
    qcw::init();
    current_monitor::init();

    let mut t_last_keepalive = time::micros();

    let mut qcw_params = QcwParameters {
        delay_compensation_ns: 150,
        startup_frequency_khz: 515.0,
        lock_range_khz: 60.0,

        run_mode: RunMode::OpenLoop,

        ontime_us: 100,
        offtime_ms: 1000,

        startup_time_us: 2,
        lock_time_us: 20,
        min_lock_current: 0.0,

        ramp_start_power: 0.1,
        ramp_end_power: 0.4,
        flat_power: 0.3,

        current_limit: 1000.0,
    };

    let mut qcw_stats = QcwStats {
        max_primary_current: 0.0,
        feedback_frequency_khz: 0.0,
    };

    let mut running = false;
    let mut on = false;
    let mut t_state_start = 0;
    let mut locked = false;

    unsafe { cortex_m::interrupt::enable() };

    let mut link = SerialLink::new();

    let mut inbox = VecDeque::new();
    let mut outbox = VecDeque::new();

    loop {
        let t_now = time::micros();
        let primary_current = current_monitor::get_current();
        let feedback_measurement = with_devices_mut(|devices, _| {
            qcw::read_capture_timer(devices)
        });

        let dt_last_keepalive = t_now - t_last_keepalive;

        if running {
            let stop = 
                (primary_current >= qcw_params.current_limit) ||
                (dt_last_keepalive >= REMOTE_TIMEOUT_US);
            if stop {
                running = false;
            }
        }
        if let Some(update_status) = update_runmode(&qcw_params, running, &mut on, &mut locked, t_now, &mut t_state_start, feedback_measurement.clone()) {
            match update_status {
                UpdateStatus::LockFailed => {
                    outbox.push_back(RemoteMessage::LockFailed);
                }
            }
        }

        qcw_stats.max_primary_current = qcw_stats.max_primary_current.max(primary_current);
        if let Some(measurement) = feedback_measurement {
            let frequency = 400_000.0 / measurement as f32;
            qcw_stats.feedback_frequency_khz = frequency
        }

        let mailbox = SerialMailbox {
            inbox: &mut inbox,
            outbox: &mut outbox,
        };
        _ = link.update(mailbox);
        while let Some(message) = inbox.pop_front() {
            match message {
                ControllerMessage::SetDebugLed(state) => debug_led::set(state),
                ControllerMessage::Ping(seq) => {
                    outbox.push_back(RemoteMessage::Ping(seq));
                },
                ControllerMessage::SetParam(param_value) => {
                    match param_value {
                        ParameterValue::DelayCompensationNS(value) =>
                            qcw_params.delay_compensation_ns = value,
                        ParameterValue::StartupFrequencykHz(frequency) =>
                            qcw_params.startup_frequency_khz = frequency,
                        ParameterValue::LockRangekHz(lock_range) =>
                            qcw_params.lock_range_khz = lock_range,
                        ParameterValue::RunMode(run_mode) =>
                            qcw_params.run_mode = run_mode,
                        ParameterValue::OnTimeUs(on_time) =>
                            qcw_params.ontime_us = on_time as u64,
                        ParameterValue::OffTimeMs(off_time) =>
                            qcw_params.offtime_ms = off_time as u64,
                        ParameterValue::StartupTimeUs(startup_time) =>
                            qcw_params.startup_time_us = startup_time as u64,
                        ParameterValue::LockTimeUs(lock_time) =>
                            qcw_params.lock_time_us = lock_time as u64,
                        ParameterValue::RampStartPower(power) =>
                            qcw_params.ramp_start_power = power,
                        ParameterValue::RampEndPower(power) =>
                            qcw_params.ramp_end_power = power,
                        ParameterValue::MinLockCurrentA(current) =>
                            qcw_params.min_lock_current = current,
                        ParameterValue::CurrentLimitA(current) =>
                            qcw_params.current_limit = current,
                        ParameterValue::FlatPower(power) =>
                            qcw_params.flat_power = power,
                    }
                }
                ControllerMessage::GetParam(param) => {
                    let param_value= match param {
                        Parameter::DelayCompensation => Some(ParameterValue::DelayCompensationNS(qcw_params.delay_compensation_ns)),
                        Parameter::StartupFrequency => Some(ParameterValue::StartupFrequencykHz(qcw_params.startup_frequency_khz)),
                        Parameter::LockRange => Some(ParameterValue::LockRangekHz(qcw_params.lock_range_khz)),
                        Parameter::RunMode => Some(ParameterValue::RunMode(qcw_params.run_mode)),
                        Parameter::OnTime => Some(ParameterValue::OnTimeUs(qcw_params.ontime_us as u16)),
                        Parameter::OffTime => Some(ParameterValue::OffTimeMs(qcw_params.offtime_ms as u16)),
                        Parameter::StartupTime => Some(ParameterValue::StartupTimeUs(qcw_params.startup_time_us as u16)),
                        Parameter::LockTime => Some(ParameterValue::LockTimeUs(qcw_params.lock_time_us as u16)),
                        Parameter::RampStartPower => Some(ParameterValue::RampStartPower(qcw_params.ramp_start_power)),
                        Parameter::RampEndPower => Some(ParameterValue::RampEndPower(qcw_params.ramp_end_power)),
                        Parameter::MinLockCurrent => Some(ParameterValue::MinLockCurrentA(qcw_params.min_lock_current)),
                        Parameter::CurrentLimit => Some(ParameterValue::CurrentLimitA(qcw_params.current_limit)),
                        Parameter::FlatPower => Some(ParameterValue::FlatPower(qcw_params.flat_power)),
                    };
                    if let Some(value) = param_value {
                        outbox.push_back(RemoteMessage::GetParamResult(value));
                    };
                }
                ControllerMessage::GetStat(stat) => {
                    let stat_value = match stat {
                        Statistic::MaxPrimaryCurrent => StatisticValue::MaxPrimaryCurrentA(qcw_stats.max_primary_current),
                        Statistic::FeedbackFrequency => StatisticValue::FeedbackFrequencykHz(qcw_stats.feedback_frequency_khz),
                    };
                    outbox.push_back(RemoteMessage::GetStatResult(stat_value));
                },
                ControllerMessage::KeepAlive => {
                    t_last_keepalive = t_now;
                },
                ControllerMessage::Run => {
                    t_state_start = t_now;
                    t_last_keepalive = t_now;
                    on = false;
                    running = true;
                },
                ControllerMessage::Stop => {
                    running = false;
                },
                ControllerMessage::ResetStats => {
                    qcw_stats = QcwStats {
                        max_primary_current: 0.0,
                        feedback_frequency_khz: 0.0
                    }
                },
                //_ => {},
            }
        }
    }
    //loop {}
}

enum UpdateStatus {
    LockFailed,
}

fn update_runmode(qcw_params: &QcwParameters, running: bool, on: &mut bool, locked: &mut bool, t_now: u64, t_state_start: &mut u64, feedback_measurement: Option<u16>) -> Option<UpdateStatus> {
    if running {
        match qcw_params.run_mode {
            RunMode::TestClosedLoop => {
                match *on {
                    true => {
                        let dt_state = t_now - *t_state_start;
                        if dt_state >= qcw_params.ontime_us {
                            debug_led::set(false);
                            with_devices_mut(|devices, cs| {
                                qcw::configure_signal_path(devices, cs, SignalPathConfig::Disabled);
                            });
                            *t_state_start = t_now;
                            *on = false;
                            *locked = false;
                        } else if !*locked && dt_state >= qcw_params.startup_time_us {
                            if dt_state >= qcw_params.lock_time_us {
                                debug_led::set(false);
                                with_devices_mut(|devices, cs| {
                                    qcw::configure_signal_path(devices, cs, SignalPathConfig::Disabled);
                                });
                                *t_state_start = t_now;
                                *on = false;
                                return Some(UpdateStatus::LockFailed);
                            } else {
                                if let Some(measurement) = feedback_measurement {
                                    let measured_frequency_khz = 400_000.0 / measurement as f32;
                                    if (qcw_params.startup_frequency_khz - measured_frequency_khz).abs() < qcw_params.lock_range_khz {
                                        debug_led::set(true);
                                        with_devices_mut(|devices, cs| {
                                            qcw::configure_signal_path(devices, cs, SignalPathConfig::ClosedLoop {
                                                period_clocks: measurement,
                                                power_profile: qcw::ClosedLoopPowerProfile::Constant(qcw_params.flat_power),
                                                delay_compensation_clocks: ((qcw_params.delay_compensation_ns as i64 * 400_000_000) / 1_000_000_000) as i16,
                                            });
                                        });
                                    }
                                }
                            }
                        }
                    },
                    false => {
                        let dt_state = t_now - *t_state_start;
                        if dt_state >= qcw_params.offtime_ms * 1000 {
                            with_devices_mut(|devices, cs| {
                                qcw::configure_signal_path(devices, cs, SignalPathConfig::OpenLoop {
                                    period_clocks: (400_000.0 / qcw_params.startup_frequency_khz) as u16,
                                    conduction_angle: qcw_params.flat_power * 0.5
                                });
                            });
                            *t_state_start = t_now;
                            *on = true;
                            *locked = false;
                        }
                    }
                }
            },
            RunMode::ClosedLoopRamp => {
                match *on {
                    true => {
                        let dt_state = t_now - *t_state_start;
                        if dt_state >= qcw_params.ontime_us {
                            debug_led::set(false);
                            with_devices_mut(|devices, cs| {
                                qcw::configure_signal_path(devices, cs, SignalPathConfig::Disabled);
                            });
                            *t_state_start = t_now;
                            *on = false;
                            *locked = false;
                        } else if !*locked && dt_state >= qcw_params.startup_time_us {
                            if dt_state >= qcw_params.lock_time_us {
                                debug_led::set(false);
                                with_devices_mut(|devices, cs| {
                                    qcw::configure_signal_path(devices, cs, SignalPathConfig::Disabled);
                                });
                                *t_state_start = t_now;
                                *on = false;
                                return Some(UpdateStatus::LockFailed);
                            } else {
                                if let Some(measurement) = feedback_measurement {
                                    let measured_frequency_khz = 400_000.0 / measurement as f32;
                                    if (qcw_params.startup_frequency_khz - measured_frequency_khz).abs() < qcw_params.lock_range_khz {
                                        *locked = true;
                                        *t_state_start = t_now;
                                        debug_led::set(true);
                                        with_devices_mut(|devices, cs| {
                                            qcw::configure_signal_path(devices, cs, SignalPathConfig::ClosedLoop {
                                                period_clocks: measurement,
                                                power_profile: qcw::ClosedLoopPowerProfile::Ramp {
                                                    start: qcw_params.ramp_start_power,
                                                    end: qcw_params.ramp_end_power,
                                                    t_start: t_now,
                                                    t_ramp: qcw_params.ontime_us,
                                                },
                                                delay_compensation_clocks: ((qcw_params.delay_compensation_ns as i64 * 400_000_000) / 1_000_000_000) as i16,
                                            });
                                        });
                                    }
                                }
                            }
                        }
                    },
                    false => {
                        let dt_state = t_now - *t_state_start;
                        if dt_state >= qcw_params.offtime_ms * 1000 {
                            with_devices_mut(|devices, cs| {
                                qcw::configure_signal_path(devices, cs, SignalPathConfig::OpenLoop {
                                    period_clocks: (400_000.0 / qcw_params.startup_frequency_khz) as u16,
                                    conduction_angle: qcw_params.flat_power * 0.5
                                });
                            });
                            *t_state_start = t_now;
                            *on = true;
                            *locked = false;
                        }
                    }
                }
            },
            RunMode::OpenLoop => {
                *locked = false;
                match *on {
                    true => {
                        let dt_state = t_now - *t_state_start;
                        if dt_state >= qcw_params.ontime_us {
                            with_devices_mut(|devices, cs| {
                                qcw::configure_signal_path(devices, cs, SignalPathConfig::Disabled);
                            });
                            *t_state_start = t_now;
                            *on = false;
                        }
                    },
                    false => {
                        let dt_state = t_now - *t_state_start;
                        if dt_state >= qcw_params.offtime_ms * 1000 {
                            with_devices_mut(|devices, cs| {
                                qcw::configure_signal_path(devices, cs, SignalPathConfig::OpenLoop {
                                    period_clocks: (400_000.0 / qcw_params.startup_frequency_khz) as u16,
                                    conduction_angle: qcw_params.flat_power * 0.5
                                });
                            });
                            *t_state_start = t_now;
                            *on = true;
                        }
                    }
                }
            },
        }
    } else {
        *locked = false;
        if *on {
            with_devices_mut(|devices, cs| {
                qcw::configure_signal_path(devices, cs, SignalPathConfig::Disabled);
            });
            *on = false;
        }
    }
    None
}
