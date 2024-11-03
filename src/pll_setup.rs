use stm32h7::stm32h753::{Peripherals};

/*
Setup the system pll to generate the high frequency bus clock the HRTIM peripheral needs
*/

#[allow(unused)]
#[repr(u16)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum SystemPllSpeed {
    MHz50,
    MHz100,
    MHz200,
    MHz400,
}

pub fn setup_system_pll(peripherals: &Peripherals, speed: SystemPllSpeed) {
    unsafe {
        peripherals.RCC.cr.modify(|_, w| {
            w
                 // turn off all PLLs
                .pll1on().clear_bit()
                .pll2on().clear_bit()
                .pll3on().clear_bit()
                // and turn on the hse clock
                .hseon().set_bit()
        });
        //wait for the hse clock to be ready
        loop {
            let cr_read = peripherals.RCC.cr.read();
            if cr_read.hserdy().is_ready() && cr_read.pll1rdy().is_not_ready() {
                break;
            }
        }
        
        peripherals.RCC.pllckselr.modify(|_, w| {
            w
                // set the pll source to HSE
                .pllsrc().hse()
        });
        peripherals.RCC.pllckselr.modify(|_, w| {
            w
                // set ref1_ck divider to 2
                .divm1().bits(2)
        });
        peripherals.RCC.pllcfgr.modify(|_, w| {
            w
                // Set PLL1's input frequency range to 8-16 MHz
                .pll1rge().range8()
                // Set PLL1's VCO to the wide range (192 to 960 MHz)
                .pll1vcosel().wide_vco()
                // Set PLL1's fracen bit to zero to disable the fractional divider
                .pll1fracen().clear_bit()
                // Enabe the divider for PLL1's p_clk, and disable it for q and r
                .divp1en().set_bit()
                .divq1en().clear_bit()
                .divr1en().clear_bit()
        });
        peripherals.RCC.pll1divr.write_with_zero(|w| {
            let w = w
                // set PLL1's feedback divider to 64, giving us a VCO frequency of 800 MHz
                .divn1().bits(63);
            // set PLL1's p clock divider to give us the intended frequency
            match speed {
                SystemPllSpeed::MHz50  => w.divp1().div16(),
                SystemPllSpeed::MHz100 => w.divp1().div8(),
                SystemPllSpeed::MHz200 => w.divp1().div4(),
                SystemPllSpeed::MHz400 => w.divp1().div2(),
            }
        });
        // turn on PLL1
        peripherals.RCC.cr.modify(|_, w| {
            w.pll1on().set_bit()
        });
        // Wait for PLL1 to be ready
        loop {
            if peripherals.RCC.cr.read().pll1rdy().is_ready() {
                break;
            }
        }
    }
}

pub fn switch_cpu_to_system_pll(peripherals: &Peripherals) {
    peripherals.RCC.d1cfgr.modify(|_, w| {
        w
            // set system d1 clock divider to 1
            .d1cpre().div1()
            // set system peripheral clock divider to 2
            .hpre().div2()
    });
    
    peripherals.RCC.cfgr.modify(|_, w| {
        // set the system clock to pll1
        w.sw().pll1()
    });
    loop {
        
        if peripherals.RCC.cfgr.read().sws().is_pll1() {
            break;
        }
    }
}
