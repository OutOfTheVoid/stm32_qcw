#![allow(unused)]

use cortex_m::delay;
use stm32h7::stm32h753::Peripherals;

use crate::device_access::with_devices_mut;

/*
QCW Signal Path
---------------       

Timer B handles frequency generation during startup, and phase delay during closed-loop operation.
During startup, it's set to operate at the initial running frequency (tuned to the desired pole of
operation) in continuous mode, and generates Cmp 1 and Cmp 2 signals 180 degrees out of phase from
each other.

When we switch to closed loop operation, the feedback enable path is enabled, and Timer B is set 
to single-shot mode, retriggerable, and cmp 1 and cmp 2 are set to give the desired phase delays
for the respective bridge phases. This is where we set the phase-shift as well as the input phase
compensation. 

At all times, Timer D is setup to capture the period of the feedback signal, so we know when the
coil is oscillating, and at what frequency. This allows us to close the loop frequency wise,
while Timer B is responsible for synchronization and phase shifts.
                     
              [Feedback Enable]                      Timer B
                   *                     *------------------------------*
                   |                     |                              |
                   v                     |                      [Cmp 1] | >--------*
     * >---*--> [ AND ] >--> [ OR ] >--> | [Reset]                      |          |
[Feedback] |                    ^        |                      [Cmp 2] | >---*    |
           |                    |        |          [Period]            |     |    |
           |                    |        *------------------------------*     |    |
           |                    |                      v                      |    |
           |                    |                      |                      |    |
    *------*                    *----------------------*                      |    |
    |                 Timer D                                                 |    |
    |      *------------------------------*                                   |    |
    |      |                              |                                   |    |
    *----> | [Cpt 1]                      |                                   |    |
    |      |                              |                                   |    |
    *----> | [Reset]                      |                                   |    |
           |                              |                                   |    |
           *------------------------------*                                   |    |
                                                                              |    |
                                                                  [Trigger B] x    x [Trigger A]

Timer A and C are the output timers, and generate the output waveforms. Their half compare 1
values are updated on Timer D's compare when it signals a capture interrupt, in addition to
the phase shift timing of timer B. They use the deadtime signal generation unit to provide
differential output signals for each pair of non-inverting gate drivers. We could optionally
use deadtime if we had a discrete gate drive bridge to prevent shoot-through.

Because these timers are in one-shot retriggerable mode, if we stop getting the trigger a and
b signals, the outputs will both eventually become low on both A and B sides of the bridge.
This will allow current to ring down through the body diode of the mosfets and back into the bus
capacitance, which should be able to absorb most of the energy pretty quickly, getting us fast
turn-off like we usually want for QCW ramps.

It also protects us in the event that feedback stops working for some other reason.

                                                                  [Trigger B] x    x [Trigger A]
                                                                              |    |
                                                                              |    |
  *---------------------------------------------------------------------------+----*
  |                                                                           |
  |            Timer A                           Timer A Output               |
  |   *------------------------------*       *--------------------*           |
  |   |                              |       |                    |           |
  |   |                      [Reset] | >---> | [Reset]       [A1] | >---------+------> * [A  Out]
  *-> | [Reset]                      |       |                    |           |
      |                      [Cmp 1] | >---> | [Set]         [A2] | >---------+------> * [!A Out]
      |                              |       |                    |           |
      *------------------------------*       *--------------------*           |
                                                                              |
  *---------------------------------------------------------------------------*   
  |
  |            Timer C                           Timer C Output
  |   *------------------------------*       *--------------------*           
  |   |                              |       |                    |           
  |   |                      [Reset] | >---> | [Reset]       [C1] | >----------------> * [B  Out]
  *-> | [Reset]                      |       |                    |           
      |                      [Cmp 1] | >---> | [Set]         [C2] | >----------------> * [!B Out]
      |                              |       |                    |           
      *------------------------------*       *--------------------*           

*/

pub fn init() {
    with_devices_mut(|devices, _| {
        // Setup the output timers first, so we enable gpio in to a known-good state. Initially, pull-downs
        // on the gate driver inputs should prevent us from activating the bridge at all.
        setup_output_timers(devices);
        // setup the input capture timer
        setup_capture_timer(devices);
        // Setup the phase timer (timer b) generally.
        setup_phase_timer(devices);
        // setup the signal path as disabled initially
        configure_signal_path(devices, SignalPathConfig::Disabled);
        // Once the output timers are initialized into a known-good state, we can activate the gpio. Both
        // outputs initialize in the same state, so the bridge won't send any current through the primary
        // circuit yet.
        setup_gpio(devices);
    });
}

fn setup_gpio(devices: &mut Peripherals) {
    /*
        setup GPIO C6 and C7 to be HRTIM A1 and A2 outputs,
        push-pull, with very high speed
        */
    devices.GPIOC.moder.modify(|_, w| {
        w
            .moder6().alternate()
            .moder7().alternate()
    });
    devices.GPIOC.afrl.modify(|_, w| {
        w
            .afr6().af1()
            .afr7().af1()
    });
    devices.GPIOC.otyper.modify(|_, w| {
        w
            .ot6().push_pull()
            .ot7().push_pull()
    });
    devices.GPIOC.ospeedr.modify(|_, w| {
        w
            .ospeedr6().very_high_speed()
            .ospeedr7().very_high_speed()
    });
    /*
        setup GPIO A9 and A10 to be HRTIM C1 and C2 outputs,
        push-pull, with very high speed
        */
    devices.GPIOA.moder.modify(|_, w| {
        w
            .moder9().alternate()
            .moder10().alternate() 
    });
    devices.GPIOA.afrh.modify(|_, w| {
        w
            .afr9().af2()
            .afr10().af2()
    });
    devices.GPIOA.otyper.modify(|_, w| {
        w
            .ot9().push_pull()
            .ot10().push_pull()
    });
    devices.GPIOA.ospeedr.modify(|_, w| {
        w
            .ospeedr9().very_high_speed()
            .ospeedr10().very_high_speed()
    });
    /*
        setup GPIO D5 to be HRTIM EEV3 input, floating (pulled down externally,
        driven by feedback cmos IC)
        */
    devices.GPIOD.afrl.modify(|_, w| {
        w.afr5().af2()
    });
    devices.GPIOD.moder.modify(|_, w| {
        w.moder5().alternate()
    });
    devices.GPIOD.pupdr.modify(|_, w| {
        w.pupdr5().pull_down()
    });
}

const HRTIM_PRESCALER_1: u8 = 0b101;

fn setup_output_timers(devices: &mut Peripherals) {
    devices.HRTIM_TIMA.timacr.modify(|_, w| {
        /*
            - No prescale, we're using a timer clock of 400 MHz
            - Preload enabled, for synchronous register updates
            - Retrigger enabled, to allow for retriggering before the 
            period in the period register has elapsed
            - Update on reset, to reload new register values on period boundaries
            */
        w
            .ck_pscx().variant(HRTIM_PRESCALER_1) 
            .preen().set_bit()
            .retrig().set_bit()
            .tx_rstu().set_bit()
    });
    // no deadtime, prescaler of 1
    devices.HRTIM_TIMA.dtar.modify(|_, w| {
        w
            .dtfx().variant(0)
            .dtrx().variant(0)
            .dtprsc().variant(0b011)
    });
    devices.HRTIM_TIMA.rsta1r.modify(|_, w| {
        w.timevnt1().set_bit() // reset on timer b cmp 1
    });
    devices.HRTIM_TIMA.seta1r.modify(|_, w| {
        w.cmp1().set_bit() // set on cmp 1
    });
    devices.HRTIM_TIMA.rstar.modify(|_, w| {
        w.timbcmp1().set_bit() // reset the timer on timer b cmp1
    });
    // set the idle state of timer a outputs to be low/high on A and !A outputs respectively
    devices.HRTIM_TIMA.outar.modify(|_, w| {
        w
            .idles1().clear_bit()
            .idles2().set_bit()
            .dten().set_bit()
            .pol1().clear_bit()
            .pol2().clear_bit()
            .fault1().variant(0b00)
            .fault2().variant(0b00)
    });
    devices.HRTIM_TIMA.perar.modify(|_, w| {
        w.perx().variant(0xF000) // set period to something long enough that it won't occur while running
    });

    devices.HRTIM_TIMC.timccr.modify(|_, w| {
        /*
            - No prescale, we're using a timer clock of 400 MHz
            - Preload enabled, for synchronous register updates
            - Retrigger enabled, to allow for retriggering before the 
            period in the period register has elapsed
            - Update on reset, to reload new register values on period boundaries
            */
        w 
            .ck_pscx().variant(HRTIM_PRESCALER_1)
            .preen().set_bit()
            .retrig().set_bit()
            .tx_rstu().set_bit()
    });
    devices.HRTIM_TIMC.rstc1r.modify(|_, w| {
        w.timevnt3().set_bit() // reset on timer b cmp 2
    });
    devices.HRTIM_TIMC.setc1r.modify(|_, w| {
        w.cmp1().set_bit() // set on cmp 1
    });
    devices.HRTIM_TIMC.rstcr.modify(|_, w| {
        w.timbcmp2().set_bit() // reset the timer on timer b cmp2
    });
    // no deadtime, prescaler of 1
    devices.HRTIM_TIMC.dtcr.modify(|_, w| {
        w
            .dtfx().variant(0)
            .dtrx().variant(0)
            .dtprsc().variant(0b011)
    });

    // set the idle state of timer c outputs to be low/high on B and !B outputs respectively
    devices.HRTIM_TIMC.outcr.modify(|_, w| {
        w
            .idles1().clear_bit()
            .idles2().set_bit()
            .dten().set_bit()
            .pol1().clear_bit()
            .pol2().clear_bit()
            .fault1().variant(0b00)
            .fault2().variant(0b00)
    });
    devices.HRTIM_TIMC.percr.modify(|_, w| {
        w.perx().variant(0xF000) // set period to something long enough that it won't occur while running
    });

    // reset both timer a and timer c and update them immediately
    devices.HRTIM_COMMON.cr2.modify(|_, w| {
        w
            .tarst().set_bit()
            .tcrst().set_bit()
            .taswu().set_bit()
            .tcswu().set_bit()
    });

    // enable the outputs
    devices.HRTIM_COMMON.oenr.write(|w| {
        w
            .ta1oen().set_bit()
            .ta2oen().set_bit()
            .tc1oen().set_bit()
            .tc2oen().set_bit()
    });
    // and enable the counters
    devices.HRTIM_MASTER.mcr.modify(|_, w| {
        w
            .tacen().set_bit()
            .tccen().set_bit()
    });
}

fn setup_phase_timer(devices: &mut Peripherals) {
    // There's not much setup to do initially, since it's mostly handled in signal path configuration
    devices.HRTIM_TIMB.timbcr.modify(|_, w| {
        w
            .ck_pscx().variant(HRTIM_PRESCALER_1)
            .preen().set_bit()
            .tx_rstu().set_bit()
    });
    devices.HRTIM_COMMON.cr2.modify(|_, w| {
        w
            .tbrst().set_bit()
            .tbswu().set_bit()
    });
}

fn setup_capture_timer(devices: &mut Peripherals) {
    // set external event 3 to be gpio D5, rising edge sensetive
    devices.HRTIM_COMMON.eecr1.modify(|_, w| {
        w
            .ee3src().variant(0)
            .ee3sns().variant(1)
    });
    // setup the capture timer to measure the period of pulses on the EEV3 input
    devices.HRTIM_TIMD.timdcr.modify(|_, w| {
        w.ck_pscx().variant(HRTIM_PRESCALER_1)
        //.preen().set_bit()
        .tx_rstu().set_bit()
        .retrig().set_bit()
        .cont().set_bit()
    });
    devices.HRTIM_TIMD.cpt1dcr.modify(|_, w| {
        w.exev3cpt().set_bit()
    });
    devices.HRTIM_TIMD.rstdr.modify(|_, w| {
        w.extevnt3().set_bit()
    });
    devices.HRTIM_TIMD.perdr.modify(|_, w| w.perx().variant(0xF000));
    devices.HRTIM_TIMD.timdicr.write(|w| w.cpt1c().set_bit());
    devices.HRTIM_TIMD.timddier5.modify(|_, w| {
        w.cpt1ie().set_bit()
    });
    devices.HRTIM_MASTER.mcr.modify(|_, w| w.tdcen().set_bit());
}

pub fn read_capture_timer(devices: &mut Peripherals) -> Option<u16> {
    if devices.HRTIM_TIMD.timdisr.read().cpt1().bit_is_set() {
        let value = devices.HRTIM_TIMD.cpt1dr.read().cpt1x().bits();
        devices.HRTIM_TIMD.timdicr.write(|w| w.cpt1c().set_bit());
        Some(value)
    } else {
        None
    }
}

#[derive(Copy, Clone, Debug)]
pub enum SignalPathConfig {
    Disabled,
    OpenLoop { period_clocks: u16, conduction_angle: f32 },
    ClosedLoop { period_clocks: u16, conduction_angle: f32, delay_compensation_clocks: i16 }
}

pub fn configure_signal_path(devices: &mut Peripherals, config: SignalPathConfig) {
    match config {
        SignalPathConfig::Disabled => {
            /* 
                Disabled
                --------
                Turn off timer b, letting timers a and c settle into their end state
            */
            devices.HRTIM_MASTER.mcr.modify(|_, w| {
                w.tbcen().clear_bit()
            });
        },
        SignalPathConfig::OpenLoop { period_clocks, conduction_angle } => {
            /*
                Open Loop
                ---------
                Run timer b as a periodic timer, triggering timer a and c on 90 and 180
                degrees respectively, providing a 90 degree conduction angle. This means
                hard switching, but in theory allows a more forgiving frequency match.
            */
            // disable timer b updates
            devices.HRTIM_COMMON.cr1.modify(|_, w| w.tbudis().set_bit());
            // continuous mode, retriggerable, fixed period
            devices.HRTIM_TIMB.timbcr.modify(|_, w| {
                w
                    .cont().set_bit()
                    .retrig().set_bit()
            });

            let half_period = period_clocks / 2;
            devices.HRTIM_TIMB.perbr.modify(|_, w| w.perx().variant(period_clocks));

            // setup timings for the periodic timer
            devices.HRTIM_TIMB.cmp1br.modify(|_, w| w.cmp1x().variant(half_period));
            devices.HRTIM_TIMB.cmp2br.modify(|_, w| w.cmp2x().variant(half_period + (half_period as f32 * (1.0 - conduction_angle)) as u16));

            // setup timings for the output timers
            devices.HRTIM_TIMA.cmp1ar.modify(|_, w| w.cmp1x().variant(half_period));
            devices.HRTIM_TIMC.cmp1cr.modify(|_, w| w.cmp1x().variant(half_period));

            // update and reset it
            devices.HRTIM_COMMON.cr1.modify(|_, w| w.tbudis().clear_bit());
            devices.HRTIM_COMMON.cr2.modify(|_, w| {
                w
                    .tbrst().set_bit()
                    .tbswu().set_bit()
            });

            // and enable it
            devices.HRTIM_MASTER.mcr.modify(|_, w| w.tbcen().set_bit());
        },
        SignalPathConfig::ClosedLoop { period_clocks, conduction_angle, delay_compensation_clocks } => {
            let half_period = period_clocks / 2;

            // disable timer b updates
            devices.HRTIM_COMMON.cr1.modify(|_, w| w.tbudis().set_bit());
            // reset on external event 3 (feedback high edge)
            devices.HRTIM_TIMB.rstbr.modify(|_, w| w.extevnt3().set_bit());
            // retriggerable, not continuous
            devices.HRTIM_TIMB.timbcr.modify(|_, w| {
                w
                    .retrig().set_bit()
                    .cont().clear_bit()
            });
            // set the period to something larger than a cycle
            devices.HRTIM_TIMB.perbr.modify(|_, w| w.perx().variant(0xF000));

            // compute phase delays
            let phase_a_delay = half_period as i32 + delay_compensation_clocks as i32;
            let phase_b_delay = half_period as i32 + delay_compensation_clocks as i32;// + (half_period as f32 * (1.0 - conduction_angle)) as i32;

            // setup output timers to be period at operating frequency
            devices.HRTIM_TIMA.cmp1ar.modify(|_, w| w.cmp1x().variant(half_period));
            devices.HRTIM_TIMC.cmp1cr.modify(|_, w| w.cmp1x().variant(half_period));

            // setup timer b to trigger our output timers periodically (with a phase delay for phase b)
            // for symmetric phase delay, could subtract half the delay from a, and add half the delay to b, but for now lets keep it simple
            devices.HRTIM_TIMB.cmp1br.modify(|_, w| w.cmp1x().variant(phase_a_delay as u16));
            devices.HRTIM_TIMB.cmp2br.modify(|_, w| w.cmp2x().variant(phase_b_delay as u16));

            // update and reset it
            // and reset the feedback timer
            devices.HRTIM_COMMON.cr1.modify(|_, w| w.tbudis().clear_bit());
            devices.HRTIM_COMMON.cr2.modify(|_, w| {
                w
                    .tbswu().set_bit()
                    .tdrst().set_bit()
            });

            // and enable it
            devices.HRTIM_MASTER.mcr.modify(|_, w| w.tbcen().set_bit());
        }
    }
}

