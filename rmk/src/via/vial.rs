use core::cell::RefCell;

use byteorder::{ByteOrder, LittleEndian};
use num_enum::FromPrimitive;

use crate::{
    action::KeyAction,
    channel::FLASH_CHANNEL,
    combo::{Combo, COMBO_MAX_NUM},
    keymap::KeyMap,
    storage::{ComboData, FlashOperationMessage},
    usb::descriptor::ViaReport,
    via::keycode_convert::{from_via_keycode, to_via_keycode},
};

/// Vial communication commands. Check [vial-qmk/quantum/vial.h`](https://github.com/vial-kb/vial-qmk/blob/20d61fcb373354dc17d6ecad8f8176be469743da/quantum/vial.h#L36)
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, FromPrimitive)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u8)]
pub(crate) enum VialCommand {
    GetKeyboardId = 0x00,
    GetSize = 0x01,
    GetKeyboardDef = 0x02,
    GetEncoder = 0x03,
    SetEncoder = 0x04,
    GetUnlockStatus = 0x05,
    UnlockStart = 0x06,
    UnlockPoll = 0x07,
    Lock = 0x08,
    QmkSettingsQuery = 0x09,
    QmkSettingsGet = 0x0A,
    QmkSettingsSet = 0x0B,
    QmkSettingsReset = 0x0C,
    DynamicEntryOp = 0x0D, /* operate on tapdance, combos, etc */
    #[num_enum(default)]
    Unhandled = 0xFF,
}

/// Vial dynamic commands. Check [vial-qmk/quantum/vial.h`](https://github.com/vial-kb/vial-qmk/blob/20d61fcb373354dc17d6ecad8f8176be469743da/quantum/vial.h#L53)
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, FromPrimitive)]
#[repr(u8)]
pub(crate) enum VialDynamic {
    DynamicVialGetNumberOfEntries = 0x00,
    DynamicVialTapDanceGet = 0x01,
    DynamicVialTapDanceSet = 0x02,
    DynamicVialComboGet = 0x03,
    DynamicVialComboSet = 0x04,
    DynamicVialKeyOverrideGet = 0x05,
    DynamicVialKeyOverrideSet = 0x06,
    #[num_enum(default)]
    Unhandled = 0xFF,
}

const VIAL_PROTOCOL_VERSION: u32 = 6;
const VIAL_EP_SIZE: usize = 32;
const VIAL_COMBO_MAX_LENGTH: usize = 4;

/// Note: vial uses litte endian, while via uses big endian
pub(crate) async fn process_vial<const ROW: usize, const COL: usize, const NUM_LAYER: usize>(
    report: &mut ViaReport,
    vial_keyboard_Id: &[u8],
    vial_keyboard_def: &[u8],
    keymap: &RefCell<KeyMap<'_, ROW, COL, NUM_LAYER>>,
) {
    // report.output_data[0] == 0xFE -> vial commands
    let vial_command = VialCommand::from_primitive(report.output_data[1]);
    info!("Received vial command: {}", vial_command);
    match vial_command {
        VialCommand::GetKeyboardId => {
            debug!("Received Vial - GetKeyboardId");
            // Returns vial protocol version + vial keyboard id
            LittleEndian::write_u32(&mut report.input_data[0..4], VIAL_PROTOCOL_VERSION);
            report.input_data[4..12].clone_from_slice(vial_keyboard_Id);
        }
        VialCommand::GetSize => {
            debug!("Received Vial - GetSize");
            LittleEndian::write_u32(&mut report.input_data[0..4], vial_keyboard_def.len() as u32);
        }
        VialCommand::GetKeyboardDef => {
            debug!("Received Vial - GetKeyboardDefinition");
            let page = LittleEndian::read_u16(&report.output_data[2..4]) as usize;
            let start = page * VIAL_EP_SIZE;
            let mut end = start + VIAL_EP_SIZE;
            if end < start || start >= vial_keyboard_def.len() {
                return;
            }
            if end > vial_keyboard_def.len() {
                end = vial_keyboard_def.len();
            }
            vial_keyboard_def[start..end]
                .iter()
                .enumerate()
                .for_each(|(i, v)| {
                    report.input_data[i] = *v;
                });
            debug!(
                "Vial return: page:{} start:{} end: {}, data: {:?}",
                page, start, end, report.input_data
            );
        }
        VialCommand::GetUnlockStatus => {
            debug!("Received Vial - GetUnlockStatus");
            // Reset all data to 0xFF(it's required!)
            report.input_data.fill(0xFF);
            // Unlocked
            report.input_data[0] = 1;
            // Unlock in progress
            report.input_data[1] = 0;
        }
        VialCommand::QmkSettingsQuery => {
            report.input_data.fill(0xFF);
        }
        VialCommand::DynamicEntryOp => {
            let vial_dynamic = VialDynamic::from_primitive(report.output_data[2]);
            match vial_dynamic {
                VialDynamic::DynamicVialGetNumberOfEntries => {
                    debug!("DynamicEntryOp - DynamicVialGetNumberOfEntries");
                    // TODO: Support dynamic tap dance
                    report.input_data[0] = 0; // Tap dance entries
                    report.input_data[1] = 8; // Combo entries
                                              // TODO: Support dynamic key override
                    report.input_data[2] = 0; // Key override entries
                }
                VialDynamic::DynamicVialTapDanceGet => {
                    warn!("DynamicEntryOp - DynamicVialTapDanceGet -- to be implemented");
                    report.input_data.fill(0x00);
                }
                VialDynamic::DynamicVialTapDanceSet => {
                    warn!("DynamicEntryOp - DynamicVialTapDanceSet -- to be implemented");
                    report.input_data.fill(0x00);
                }
                VialDynamic::DynamicVialComboGet => {
                    debug!("DynamicEntryOp - DynamicVialComboGet");
                    report.input_data[0] = 0; // Index 0 is the return code, 0 means success

                    let combo_idx = report.output_data[3] as usize;
                    let combos = &keymap.borrow().combos;
                    if let Some((_, combo)) = vial_combo(combos, combo_idx) {
                        for i in 0..4 {
                            LittleEndian::write_u16(
                                &mut report.input_data[1 + i * 2..3 + i * 2],
                                to_via_keycode(*combo.actions.get(i).unwrap_or(&KeyAction::No)),
                            );
                        }
                        LittleEndian::write_u16(
                            &mut report.input_data[9..11],
                            to_via_keycode(combo.output),
                        );
                    } else {
                        report.input_data[1..11].fill(0);
                    }
                }
                VialDynamic::DynamicVialComboSet => {
                    debug!("DynamicEntryOp - DynamicVialComboSet");
                    report.input_data[0] = 0; // Index 0 is the return code, 0 means success

                    let (real_idx, actions, output) = {
                        // Drop combos to release the borrowed keymap, avoid potential run-time panics
                        let combo_idx = report.output_data[3] as usize;
                        let combos = &mut keymap.borrow_mut().combos;
                        let Some((real_idx, combo)) = vial_combo_mut(combos, combo_idx) else {
                            return;
                        };

                        let mut actions = heapless::Vec::new();
                        for i in 0..4 {
                            let action = from_via_keycode(LittleEndian::read_u16(
                                &report.output_data[4 + i * 2..6 + i * 2],
                            ));
                            if action != KeyAction::No {
                                let _ = actions.push(action);
                            }
                        }
                        let output =
                            from_via_keycode(LittleEndian::read_u16(&report.output_data[12..14]));

                        combo.actions = actions;
                        combo.output = output;

                        let mut actions = [KeyAction::No; 4];
                        for (i, &action) in combo.actions.iter().enumerate() {
                            actions[i] = action;
                        }
                        (real_idx, actions, output)
                    };
                    FLASH_CHANNEL
                        .send(FlashOperationMessage::WriteCombo(ComboData {
                            idx: real_idx,
                            actions,
                            output,
                        }))
                        .await;
                }
                VialDynamic::DynamicVialKeyOverrideGet => {
                    warn!("DynamicEntryOp - DynamicVialKeyOverrideGet -- to be implemented");
                    report.input_data.fill(0x00);
                }
                VialDynamic::DynamicVialKeyOverrideSet => {
                    warn!("DynamicEntryOp - DynamicVialKeyOverrideSet -- to be implemented");
                    report.input_data.fill(0x00);
                }
                VialDynamic::Unhandled => {
                    warn!("DynamicEntryOp - Unhandled -- subcommand not recognized");
                    report.input_data.fill(0x00);
                }
            }
        }
        VialCommand::GetEncoder => {
            let layer = report.output_data[2];
            let index = report.output_data[3];
            debug!(
                "Received Vial - GetEncoder, encoder idx: {} at layer: {}",
                index, layer
            );
            // Get encoder value
            // if let Some(encoders) = &keymap.borrow().encoders {
            //     if let Some(encoder_layer) = encoders.get(layer as usize) {
            //         if let Some(encoder) = encoder_layer.get(index as usize) {
            //             let clockwise = to_via_keycode(encoder.0);
            //             BigEndian::write_u16(&mut report.input_data[0..2], clockwise);
            //             let counter_clockwise = to_via_keycode(encoder.1);
            //             BigEndian::write_u16(&mut report.input_data[2..4], counter_clockwise);
            //             return;
            //         }
            //     }
            // }

            // Clear returned value, aka `KeyAction::No`
            report.input_data.fill(0x0);
        }
        VialCommand::SetEncoder => {
            let layer = report.output_data[2];
            let index = report.output_data[3];
            let clockwise = report.output_data[4];
            debug!(
                "Received Vial - SetEncoder, encoder idx: {} clockwise: {} at layer: {}",
                index, clockwise, layer
            );
            // if let Some(&mut mut encoders) = keymap.borrow_mut().encoders {
            //     if let Some(&mut mut encoder_layer) = encoders.get_mut(layer as usize) {
            //         if let Some(&mut mut encoder) = encoder_layer.get_mut(index as usize) {
            //             if clockwise == 1 {
            //                 let keycode = BigEndian::read_u16(&report.output_data[5..7]);
            //                 let action = from_via_keycode(keycode);
            //                 info!("Setting clockwise action: {}", action);
            //                 encoder.0 = action
            //             } else {
            //                 let keycode = BigEndian::read_u16(&report.output_data[5..7]);
            //                 let action = from_via_keycode(keycode);
            //                 info!("Setting counter-clockwise action: {}", action);
            //                 encoder.1 = action
            //             }
            //         }
            //     }
            // }
            debug!("Received Vial - SetEncoder, data: {}", report.output_data);
        }
        _ => (),
    }
}

fn vial_combo(combos: &[Combo; COMBO_MAX_NUM], idx: usize) -> Option<(usize, &Combo)> {
    combos
        .iter()
        .enumerate()
        .filter(|(_, combo)| combo.actions.len() <= VIAL_COMBO_MAX_LENGTH)
        .enumerate()
        .find_map(|(i, combo)| (i == idx).then_some(combo))
}

fn vial_combo_mut(combos: &mut [Combo; COMBO_MAX_NUM], idx: usize) -> Option<(usize, &mut Combo)> {
    combos
        .iter_mut()
        .enumerate()
        .filter(|(_, combo)| combo.actions.len() <= VIAL_COMBO_MAX_LENGTH)
        .enumerate()
        .find_map(|(i, combo)| (i == idx).then_some(combo))
}
