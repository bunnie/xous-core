#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod ux;
use ux::*;
use num_traits::*;
use xous_ipc::Buffer;
use xous::{send_message, Message};
use usbd_human_interface_device::device::fido::*;
use std::thread;

mod ctap;
use ctap::hid::{ChannelID, CtapHid};
use ctap::status_code::Ctap2StatusCode;
use ctap::CtapState;
mod shims;
use shims::*;
mod submenu;

use locales::t;

// CTAP2 testing notes:
// run our branch and use this to forward the prompts on to the device:
// netcat -k -u -l 6502 > /dev/ttyS0
// use the "autotest" feature to remove some excess prompts that interfere with the test

// the OpenSK code is based off of commit f2496a8e6d71a4e838884996a1c9b62121f87df2 from the
// Google OpenSK repository. The last push was Nov 19 2021, and the initial merge into Xous
// was finished on June 9 2022. Any patches to this code base will have to be manually
// applied. Please update the information here to reflect the latest patch status.

/*
UI concept:

  |-----------------|
  |                 |
  | List view       |
  | area            |
  |                 |
  |                 |
  |                 |
  |                 |
  |                 |
  |-----------------|
  | List filter     |
  |-----------------|
  |F1 | F2 | F3 | F4|
  |-----------------|

  F1-F4: switch between functions using F-keys. Functions are:
    - FIDO2   (U2F authenicators)
    - TOTP    (time based authenticators)
    - Vault   (passwords)
    - Prefs   (preferences)
  Tap once to switch to the sub-function.
  Once on the sub-function, tap the corresponding F-key again to raise
  the menu for that sub-function.

  List filter:
    - Any regular keys hit here appear in the search input. It automatically
      filters the content in the list view area to the set of strings that match
      the search input

  Up/down arrow: picks a list view item
  Left/right arrow: moves up or down the list view in pages
  Enter: picks the selected list view
  Select: *alaways* raises system 'main menu'
 */

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum VaultOp {
    /// a line of text has arrived
    Line = 0, // make sure we occupy opcodes with discriminants < 1000, as the rest are used for callbacks
    /// incremental line of text
    IncrementalLine,
    /// redraw our UI
    Redraw,
    /// change focus
    ChangeFocus,

    /// Menu items
    MenuAutotype,
    MenuEdit,
    MenuDelete,
    MenuChangeFont,

    /// exit the application
    Quit,
}

enum VaultMode {
    Fido,
    Totp,
    Password,
}

fn main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Debug);
    log::info!("my PID is {}", xous::process::id());

    // let's try keeping this completely private as a server. can we do that?
    let sid = xous::create_server().unwrap();
    start_fido_ux_thread();

    // spawn the FIDO2 USB handler
    let _ = thread::spawn({
        move || {
            let xns = xous_names::XousNames::new().unwrap();
            let tt = ticktimer_server::Ticktimer::new().unwrap();
            let boot_time = ClockValue::new(tt.elapsed_ms() as i64, 1000);

            let mut rng = ctap_crypto::rng256::XousRng256::new(&xns);
            // this call will block until the PDDB is mounted.
            let usb = usb_device_xous::UsbHid::new();
            let mut ctap_state = CtapState::new(&mut rng, check_user_presence, boot_time);
            let mut ctap_hid = CtapHid::new();
            let pddb = pddb::Pddb::new();
            pddb.is_mounted_blocking();
            loop {
                match usb.u2f_wait_incoming() {
                    Ok(msg) => {
                        log::trace!("FIDO listener got message: {:?}", msg);
                        let now = ClockValue::new(tt.elapsed_ms() as i64, 1000);
                        let reply = ctap_hid.process_hid_packet(&msg.packet, now, &mut ctap_state);
                        // This block handles sending packets.
                        for pkt_reply in reply {
                            let mut reply = RawFidoMsg::default();
                            reply.packet.copy_from_slice(&pkt_reply);
                            let status = usb.u2f_send(reply);
                            match status {
                                Ok(()) => {
                                    log::trace!("Sent U2F packet");
                                }
                                Err(e) => {
                                    log::error!("Error sending U2F packet: {:?}", e);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        match e {
                            xous::Error::ProcessTerminated => { // unplug happened, reset the authenticator
                                log::info!("CTAP unplug_reset");
                                ctap_state.unplug_reset();
                            },
                            _ => {
                                log::warn!("FIDO listener got an error: {:?}", e);
                            }
                        }
                    }
                }
            }
        }
    });

    let conn = xous::connect(sid).unwrap();
    // spawn the icontray handler
    let _ = thread::spawn({
        move || {
            icontray_server(conn);
        }
    });

    let menu_sid = xous::create_server().unwrap();
    let _menu_mgr = submenu::create_submenu(conn, menu_sid);

    let xns = xous_names::XousNames::new().unwrap();
    // TODO: add a UX loop that indicates we're waiting for a PDDB mount before moving forward
    let mut vaultux = VaultUx::new(&xns, sid);
    vaultux.set_mode(VaultMode::Fido);
    let mut allow_redraw = false;
    let modals = modals::Modals::new(&xns).unwrap();
    loop {
        let msg = xous::receive_message(sid).unwrap();
        log::debug!("got message {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(VaultOp::IncrementalLine) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let s = buffer.as_flat::<xous_ipc::String<4000>, _>().unwrap();
                log::info!("Incremental input: {}", s.as_str());
                vaultux.input(s.as_str()).expect("Vault couldn't accept input string");
                send_message(conn,
                    Message::new_scalar(VaultOp::Redraw.to_usize().unwrap(), 0, 0, 0, 0)
                ).ok();
            }
            Some(VaultOp::Line) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let s = buffer.as_flat::<xous_ipc::String<4000>, _>().unwrap();
                log::debug!("vaultux got input line: {}", s.as_str());
                match s.as_str() {
                    "\u{0011}" => {
                        vaultux.set_mode(VaultMode::Fido);
                    }
                    "\u{0012}" => {
                        vaultux.set_mode(VaultMode::Totp);
                    }
                    "\u{0013}" => {
                        vaultux.set_mode(VaultMode::Password);
                    }
                    "\u{0014}" => {
                        vaultux.raise_menu();
                    }
                    "↓" => {
                        vaultux.nav(NavDir::Down);
                    }
                    "↑" => {
                        vaultux.nav(NavDir::Up);
                    }
                    "←" => {
                        vaultux.nav(NavDir::PageUp);
                    }
                    "→" => {
                        vaultux.nav(NavDir::PageDown);
                    }
                    _ => {
                        // someone hit enter. The string is the whole search query, but what we care is that someone hit enter.
                        vaultux.raise_menu();
                    }
                }
                send_message(conn,
                    Message::new_scalar(VaultOp::Redraw.to_usize().unwrap(), 0, 0, 0, 0)
                ).ok();
            }
            Some(VaultOp::Redraw) => {
                if allow_redraw {
                    vaultux.redraw().expect("Vault couldn't redraw");
                }
            }
            Some(VaultOp::ChangeFocus) => xous::msg_scalar_unpack!(msg, new_state_code, _, _, _, {
                let new_state = gam::FocusState::convert_focus_change(new_state_code);
                match new_state {
                    gam::FocusState::Background => {
                        allow_redraw = false;
                    }
                    gam::FocusState::Foreground => {
                        allow_redraw = true;
                    }
                }
                /*
                xous::yield_slice();
                send_message(conn,
                    Message::new_scalar(VaultOp::Redraw.to_usize().unwrap(), 0, 0, 0, 0)
                ).ok(); */
            }),
            Some(VaultOp::MenuAutotype) => {
                log::info!("got autotype");
            },
            Some(VaultOp::MenuDelete) => {
                log::info!("got delete");
            },
            Some(VaultOp::MenuEdit) => {
                log::info!("got edit");
            }
            Some(VaultOp::MenuChangeFont) => {
                for item in FONT_LIST {
                    modals
                        .add_list_item(item)
                        .expect("couldn't build radio item list");
                }
                match modals.get_radiobutton(t!("vault.select_font", xous::LANG)) {
                    Ok(style) => {
                        vaultux.set_glyph_style(name_to_style(&style).unwrap_or(DEFAULT_FONT));
                    },
                    _ => log::error!("get_radiobutton failed"),
                }
            }
            Some(VaultOp::Quit) => {
                log::error!("got Quit");
                break;
            }
            _ => {
                log::trace!("got unknown message {:?}", msg);
            }
        }
        log::trace!("reached bottom of main loop");
    }
    // clean up our program
    log::error!("main loop exit, destroying servers");
    xns.unregister_server(sid).unwrap();
    xous::destroy_server(sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}

fn check_user_presence(_cid: ChannelID) -> Result<(), Ctap2StatusCode> {
    log::warn!("check user presence called, but not implemented!");
    Ok(())
}
