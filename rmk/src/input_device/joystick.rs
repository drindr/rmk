use crate::event::{AxisEvent, AxisValType};
use crate::keyboard::EVENT_CHANNEL_SIZE;
use crate::usb::descriptor::{CompositeReport, CompositeReportType};
use crate::REPORT_CHANNEL_SIZE;
use embassy_futures::select::{select3, Either3};

use super::{adc::AnalogChannel, DeviceReceiver, DeviceSender, InputDevice, InputProcessor};
pub use crate::event::Event;
use crate::keyboard::{KeyboardReportMessage, EVENT_CHANNEL, KEYBOARD_REPORT_CHANNEL};

/// Joystick
///
struct Joystick<X, Y, Z> {
    x: X,
    y: Y,
    z: Z,
}

impl<'a, X: AnalogChannel, Y: AnalogChannel, Z: AnalogChannel> InputDevice<'a>
    for Joystick<X, Y, Z>
{
    type EventType = Event;
    type SenderType = DeviceSender<'a, Self::EventType, EVENT_CHANNEL_SIZE>;

    async fn run(&mut self) {
        let mut report = [
            AxisEvent {
                typ: AxisValType::Rel,
                axis: crate::event::Axis::X,
                value: 0,
            },
            AxisEvent {
                typ: AxisValType::Rel,
                axis: crate::event::Axis::Y,
                value: 0,
            },
            AxisEvent {
                typ: AxisValType::Rel,
                axis: crate::event::Axis::Z,
                value: 0,
            },
        ];
        loop {
            match select3(self.x.read(), self.y.read(), self.z.read()).await {
                Either3::First(x) => {
                    report[0].value = x;
                }
                Either3::Second(y) => {
                    report[1].value = y;
                }
                Either3::Third(z) => {
                    report[2].value = z;
                }
            }
            self.event_sender().send(Event::Joystick(report)).await;
        }
    }

    fn event_sender(&self) -> Self::SenderType {
        return EVENT_CHANNEL.sender();
    }
}

pub struct JoystickProcessor {
    transform: Option<[[i8; 3]; 3]>,
    bound: [[i16; 2]; 3],
    report: CompositeReport,
}

impl<'a, 'b> InputProcessor<'a, 'b> for JoystickProcessor {
    type EventType = Event;
    type ReportType = KeyboardReportMessage;
    type SenderType = DeviceSender<'a, Self::ReportType, REPORT_CHANNEL_SIZE>;
    type ReceiverType = DeviceReceiver<'b, Self::EventType, EVENT_CHANNEL_SIZE>;

    async fn process(&mut self, event: Self::EventType) {
        let mut val = [0i8; 3];
        match event {
            Event::Joystick(mut axis_data) => {
                axis_data
                    .iter_mut()
                    .zip(self.bound.iter())
                    .for_each(|(axis, bound)| {
                        if axis.value < bound[0] {
                            axis.value = bound[0];
                        } else if axis.value > bound[1] {
                            axis.value = bound[1];
                        }
                    });
                match self.transform {
                    None => {
                        val.iter_mut().zip(axis_data.iter()).for_each(|(r, s)| {
                            *r = (s.value / 256) as i8;
                        });
                    }
                    Some(trans) => {
                        val.iter_mut().zip(trans.iter()).for_each(|(r, axis)| {
                            let mut value = 0i8;
                            axis_data.iter().zip(axis).for_each(|(s, p)| {
                                let v = (s.value / 256) as i8;
                                value += v * p;
                            });
                            *r = value;
                        });
                    }
                };
            }
            _ => {}
        };
        self.report.x = val[0];
        self.report.y = val[1];
        self.report_sender()
            .send(KeyboardReportMessage::CompositeReport(
                self.report,
                CompositeReportType::Mouse,
            ))
            .await;
    }

    fn event_receiver(&self) -> Self::ReceiverType {
        return EVENT_CHANNEL.receiver();
    }
    fn report_sender(&self) -> Self::SenderType {
        return KEYBOARD_REPORT_CHANNEL.sender();
    }
}
