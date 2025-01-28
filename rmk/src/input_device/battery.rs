use super::DeviceReceiver;

#[allow(unused_imports)]
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};

#[allow(unused_imports)]
use super::{
    adc::{AnalogReceiver, ANALOG_CHANNEL_SIZE},
    DeviceSender, InputProcessor,
};
pub const BATTERY_CHANNEL_SIZE: usize = 8;
pub type BatteryReceiver<'a> = DeviceReceiver<'a, u8, BATTERY_CHANNEL_SIZE>;

#[cfg(feature = "_nrf")]
pub struct BatteryProcessor<'a> {
    report_channel: Channel<CriticalSectionRawMutex, u8, BATTERY_CHANNEL_SIZE>,
    event_channel: AnalogReceiver<'a>,
    adc_divider_measured: u32,
    adc_divider_total: u32,
}

#[cfg(feature = "_nrf")]
impl BatteryProcessor<'_> {
    fn get_battery_percent(&self, val: i16) -> u8 {
        info!("Detected adc value: {:?}", val);
        // Avoid overflow
        let val = val as i32;

        // According to nRF52840's datasheet, for single_ended saadc:
        // val = v_adc * (gain / reference) * 2^(resolution)
        //
        // When using default setting, gain = 1/6, reference = 0.6v, resolution = 12bits, so:
        // val = v_adc * 1137.8
        //
        // For example, rmk-ble-keyboard uses two resistors 820K and 2M adjusting the v_adc, then,
        // v_adc = v_bat * measured / total => val = v_bat * 1137.8 * measured / total
        //
        // If the battery voltage range is 3.6v ~ 4.2v, the adc val range should be (4096 ~ 4755) * measured / total
        let mut measured = self.adc_divider_measured as i32;
        let mut total = self.adc_divider_total as i32;
        if 500 < val && val < 1000 {
            // Thing becomes different when using vddh as reference
            // The adc value for vddh pin is actually vddh/5,
            // so we use this rough range to detect vddh
            measured = 1;
            total = 5;
        }
        if val > 4755_i32 * measured / total {
            // 4755 ~= 4.2v * 1137.8
            100_u8
        } else if val < 4055_i32 * measured / total {
            // 4096 ~= 3.6v * 1137.8
            // To simplify the calculation, we use 4055 here
            0_u8
        } else {
            ((val * total / measured - 4055) / 7) as u8
        }
    }
}

#[cfg(feature = "_nrf")]
impl<'a> InputProcessor<'a, 'a> for BatteryProcessor<'a> {
    type EventType = i16;
    type ReportType = u8;
    type ReceiverType = AnalogReceiver<'a>;
    type SenderType = DeviceSender<'a, Self::ReportType, BATTERY_CHANNEL_SIZE>;

    async fn process(&mut self, event: Self::EventType) {
        let battery_percent = self.get_battery_percent(event);
        self.report_sender().send(battery_percent).await;
    }
    fn event_receiver(&self) -> Self::ReceiverType {
        self.event_channel
    }
    fn report_sender(&'a self) -> Self::SenderType {
        self.report_channel.sender()
    }
}
