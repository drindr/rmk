//! ADC: Nrf's SAADC.

#[allow(unused_imports)]
use super::{DeviceReceiverMarker, DeviceSender, DeviceSenderMarker, InputDevice};
use core::future::Future;
#[allow(unused_imports)]
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
pub const ANALOG_CHANNEL_SIZE: usize = 8;

use super::DeviceReceiver;

pub type AnalogReceiver<'a> = DeviceReceiver<'a, i16, ANALOG_CHANNEL_SIZE>;

pub trait AnalogChannel {
    fn read(&self) -> impl Future<Output = i16>;
}

impl AnalogChannel for AnalogReceiver<'_> {
    async fn read(&self) -> i16 {
        return self.receive().await;
    }
}

trait AnalogDevice {
    fn run(&mut self) -> impl Future<Output = ()>;
    fn get_channel(&self, id: usize) -> AnalogReceiver;
}

#[cfg(feature = "_nrf")]
struct SaadcDevice<'a, const N: usize> {
    saadc: embassy_nrf::saadc::Saadc<'a, N>,
    channels: [Channel<CriticalSectionRawMutex, i16, ANALOG_CHANNEL_SIZE>; N],
}

#[cfg(feature = "_nrf")]
impl<'a, 'b, const N: usize> InputDevice<'b> for SaadcDevice<'a, N> {
    type EventType = i16;
    type SenderType = DeviceSender<'b, Self::EventType, ANALOG_CHANNEL_SIZE>;
    async fn run(&mut self) {
        <Self as AnalogDevice>::run(self).await;
    }

    fn event_sender(&'b self) -> Self::SenderType {
        // return the first channel's sender as default
        self.channels[0].sender()
    }
}

#[cfg(feature = "_nrf")]
impl<'a, const N: usize> AnalogDevice for SaadcDevice<'a, N> {
    async fn run(&mut self) {
        let mut dma_buf = [0i16; N];
        loop {
            self.saadc.sample(&mut dma_buf).await;
            dma_buf
                .iter()
                .zip(self.channels.iter())
                .for_each(|(val, chan)| match chan.try_send(*val) {
                    Ok(_) => (),
                    Err(_) => {
                        warn!("Analog channel is full");
                    }
                });
        }
    }
    fn get_channel(&self, id: usize) -> AnalogReceiver {
        self.channels[id].receiver()
    }
}
