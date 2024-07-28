#![forbid(unsafe_code)] 
#![warn(missing_docs)]
#![allow(clippy::new_without_default)]
//! A simple Modbus engine component.
//!
//! This engine implements coils and registers for use in a multi-threaded environment. It is meant
//! to provide part of a larger application system, such as a Modbus TCP server or similar.
//!
//! ```
//! use simplemodbus::*;
//! let mut core = MbCore::new();
//! let core_cmd_tx = core.get_channel();
//! let handle = std::thread::spawn(move || core.process());
//!
//! // setup a channel pair
//! let (tx, rx) = std::sync::mpsc::channel();
//! let packet = MbPacket {
//!     addr: 99,
//!     value: MbValue::SingleCoil(true)
//! };
//! //send the command to write a single coil
//! core_cmd_tx.send(MbMessage {
//!     cmd: MbMessageCmd::WriteSingleCoil(packet),
//!     reply_tx: Some(tx)
//! }).expect("Failed to send message.");
//!
//! // check for a reply to confirm command success
//! // this can be skipped
//! // Replies follow the Modbus protocol specification, i.e. echoing write commands
//! // or returning values for reads.
//! let cmd_reply = rx.recv().unwrap();
//! if let MbMessageReply::WriteSingleCoil(packet) = cmd_reply {
//!    if let MbValue::SingleCoil(reply_coil) = packet.value {
//!        assert_eq!(packet.addr, 99);
//!        assert_eq!(reply_coil, true);
//!    }
//!    else { panic!("Received unexpected message value type."); }
//! }
//! else { panic!("Received unexpected message type."); }
//!
//! let (tx, rx) = std::sync::mpsc::channel();
//! // send the command to read a single coil 
//! // we read the one we set previously
//! let extent = MbExtent {
//!     addr: 99,
//!     quantity: 1
//! };
//! core_cmd_tx.send(MbMessage {
//!     cmd: MbMessageCmd::ReadCoils(extent),
//!     reply_tx: Some(tx)
//! }).expect("Failed to send message.");
//!
//! let reply = rx.recv().unwrap();
//! if let MbMessageReply::ReadCoils(packet) = reply {
//!    if let MbValue::MultiCoil(reply_coils) = packet.value {
//!        assert_eq!(packet.addr, 99);
//!        assert_eq!(reply_coils.len(), 1);
//!        assert_eq!(reply_coils[0], true);
//!    }
//!    else { panic!("Received unexpected message value type."); }
//! }
//! else { panic!("Received unexpected message type."); }
//! ```


pub mod mbcore;
pub mod mbtypes;

pub use mbcore::*;
pub use mbtypes::*;

/// A demonstration function.
///
/// demo() will fire up a "noisemaker" thread, a "reader" thread, and the MbCore processor thread.
/// When run, it will run for a period of time, outputting a line of coils and registers
/// corresponding to a representation of the current time.
pub fn demo() {
    use std::io::prelude::*;
    use std::{thread, time, time::Duration};
    use std::sync::mpsc;

    let mut mbc = MbCore::new();
    let cmd_tx_og = mbc.get_channel();

    let processor_handle = thread::spawn(move || mbc.process());

    let cmd_tx = cmd_tx_og.clone();
    let noisemaker_handle = thread::spawn(move || {
        let (tx, _) = mpsc::channel();

        for _ in 0..500 {
            let now = time::SystemTime::now().duration_since(time::SystemTime::UNIX_EPOCH).unwrap().as_nanos();
            for i in 0..32 {
                let bit = (now >> i) & 0x01 == 1;
                let packet = MbPacket {
                    addr: i,
                    value: MbValue::SingleCoil(bit)
                };
                cmd_tx.send(MbMessage {
                    cmd: MbMessageCmd::WriteSingleCoil(packet),
                    reply_tx: Some(tx.clone())
                }).expect("noisemaker co cmd send");
            }

            for i in 0..5 {
                let word: u16 = (now >> (i*16)) as u16;
                let packet = MbPacket {
                    addr: i,
                    value: MbValue::SingleReg(word)
                };
                cmd_tx.send(MbMessage {
                    cmd: MbMessageCmd::WriteSingleHoldingReg(packet),
                    reply_tx: Some(tx.clone())
                }).expect("noisemaker hr cmd send");
            }
            thread::sleep(Duration::from_millis(100));
        }
        println!("\nNoisemaker done!");
    });

    let cmd_tx = cmd_tx_og.clone();
    let reader_handle = thread::spawn(move || {
        let (tx, rx) = mpsc::channel();

        loop {
            let coil_extent = MbExtent {
                addr: 0,
                quantity: 32
            };
            let send_res = cmd_tx.send(MbMessage {
                cmd: MbMessageCmd::ReadCoils(coil_extent),
                reply_tx: Some(tx.clone())
            });
            if send_res.is_err() {
                break;
            }
            let reg_extent = MbExtent {
                addr: 0,
                quantity: 5
            };
            let send_res = cmd_tx.send(MbMessage {
                cmd: MbMessageCmd::ReadHoldingRegs(reg_extent),
                reply_tx: Some(tx.clone())
            });
            if send_res.is_err() {
                break;
            }

            while let Ok(reply) = rx.try_recv() {
                match reply {
                    MbMessageReply::ReadCoils(packet) => {
                        if let MbValue::MultiCoil(reply_coils) = packet.value {
                            print!("\r");
                            for c in reply_coils {
                                //let bit: u8 = c.into();
                                let bit = if c {
                                    '*'
                                }
                                else {
                                    '_'
                                };
                                print!("{}", bit);
                                let _ = std::io::stdout().flush();
                            }
                            let _ = std::io::stdout().flush();
                        }
                    },

                    MbMessageReply::ReadHoldingRegs(packet) => {
                        if let MbValue::MultiReg(reply_regs) = packet.value {
                            for c in reply_regs {
                                print!(" {:04x}", c);
                                let _ = std::io::stdout().flush();
                            }
                        }
                    },
                    _ => break
                }
            }
            let _ = std::io::stdout().flush();
            thread::sleep(Duration::from_millis(5));
        }
        println!("\nReader done!");
    });

    let _ = noisemaker_handle.join();

    cmd_tx_og.send(MbMessage {
        cmd: MbMessageCmd::Shutdown,
        reply_tx: None
    }).unwrap();
    let _ = processor_handle.join();

    let _ = reader_handle.join();
}



#[cfg(test)]
mod tests {
    // use super::*;

    #[test]
    fn it_works() {
    }
}
