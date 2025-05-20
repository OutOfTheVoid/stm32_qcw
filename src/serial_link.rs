use alloc::collections::vec_deque::VecDeque;

use qcw_com::*;

use super::device_access::with_devices_mut;

pub struct SerialLink {
    rx_buffer: SerialBuffer<512>,
    tx_buffer: SerialBuffer<512>,
}

pub struct SerialMailbox<'a> {
    pub inbox: &'a mut VecDeque<ControllerMessage>,
    pub outbox: &'a mut VecDeque<RemoteMessage>,
}

// usart_ker_ck is 200 MHz

impl SerialLink {
    pub fn new() -> Self {
        with_devices_mut(|devices, _| {
            // PA2 -> USART2_TX, push-pull output, medium speed
            // PA3 -> USART2_RX, floating input
            devices.GPIOA.moder.modify(|_, w| {
                w
                    .moder2().alternate()
                    .moder3().alternate()
            });
            devices.GPIOA.afrl.modify(|_, w| {
                w
                    .afr2().af7()
                    .afr3().af7()
            });
            devices.GPIOA.otyper.modify(|_, w| w.ot2().push_pull());
            devices.GPIOA.ospeedr.modify(|_, w| w.ospeedr2().medium_speed());
            devices.GPIOA.pupdr.modify(|_, w| w.pupdr3().pull_up());

            // setup as tx/rx, no interrupts, no mute mode, 8 bits, no parity
            devices.USART2.cr1.write(|w| {
                w
                    .fifoen().set_bit()
                    .m0().clear_bit()
                    .m1().clear_bit()
                    .over8().oversampling16()
                    .mme().clear_bit()
                    .pce().clear_bit()
                    .re().set_bit()
                    .te().set_bit()
            });
            // setup prescaler as 1/32, giving a baud clock of 6_250_000 Hz
            devices.USART2.presc.write(|w| w.prescaler().variant(0b1000));
            // request clear of the fifo
            devices.USART2.brr.write(|w| {
                w.brr().variant(625)
            });
            // clear rx and tx fifos
            devices.USART2.rqr.write(|w| {
                w
                    .rxfrq().set_bit()
            });
            // enable the uart
            devices.USART2.cr1.modify(|_, w| w.ue().set_bit());
        });
        SerialLink {
            tx_buffer: SerialBuffer::new(),
            rx_buffer: SerialBuffer::new(),
        }
    }

    pub fn update(&mut self, mailbox: SerialMailbox<'_>) -> Result<(), ()> {
        
        with_devices_mut(|devices, _| {
            while devices.USART2.isr.read().rxne().bit_is_set() && self.rx_buffer.free_space() != 0 {
                let byte = (devices.USART2.rdr.read().rdr().bits() & 0xFF) as u8;
                self.rx_buffer.push(byte);
            }
            Ok(())
        })?;
        
        while let Some(message) = ControllerMessage::try_receive(&mut self.rx_buffer)? {
            mailbox.inbox.push_back(message);
        }

        while self.tx_buffer.free_space() != 0 {
            if let Some(outgoing) = mailbox.outbox.front() {
                if outgoing.try_send(&mut self.tx_buffer) {
                    mailbox.outbox.pop_front();
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        with_devices_mut(|devices, _| {
            while devices.USART2.isr.read().txe().bit_is_set() && self.tx_buffer.count() != 0 {
                let byte = self.tx_buffer.pop().unwrap();
                devices.USART2.tdr.write(|w| w.tdr().variant(byte as u16));
            }
            Ok(())
        })?;

        Ok(())
    }
}
