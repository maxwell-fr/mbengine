//! Core of the Modbus engine.
//!
//! MbCore implements a simple request-reply system over sync::mpsc channels. The core component is
//! intended to be a backing system for a Modbus interface presented over TCP/IP or another
//! protocol.

use std::{sync::mpsc, thread, time::Duration};

use crate::mbtypes::*;

/// Provides data and functionality for a simulated Modbus device.
pub struct MbCore {
    coils: [bool; 65536], // 0x0 to 0xffff 1-bit coils
    holding_regs: [u16; 65536], // 0x0 to 0xffff, 64Ki registers
    // future expansions could add input registers and discretes
    cmd_rx: mpsc::Receiver<MbMessage>,
    cmd_tx: mpsc::Sender<MbMessage>,
}


impl MbCore {
    const NUM_COILS: usize = 65536;
    const NUM_REGS: usize = 65536;
    const MAX_COIL: usize = Self::NUM_COILS - 1;
    const MAX_REG: usize = Self::NUM_REGS - 1;

    /// Initialize a new MbCore
    pub fn new() -> Self {
        let (channel_tx, channel_rx) = mpsc::channel();

        MbCore {
            coils: [false; Self::NUM_COILS],
            holding_regs: [0; Self::NUM_REGS],
            cmd_rx: channel_rx,
            cmd_tx: channel_tx,
        }
    }


    /// Bulk load coils or registers.
    ///
    /// Unlike the regular WriteMultipleX commands, this function is not constrained by protocol
    /// length limitations.
    ///
    /// Returns an error if the provided MbValue is not a Multi-type value or
    /// if the number of registers plus the offset is greater than NUM_COILS.
    pub fn load_bulk(&mut self, packet: MbPacket) -> Result<(), MbFuncError> {
        match packet.value {
            MbValue::MultiReg(regs) => {
                if regs.len() + packet.addr as usize > Self::MAX_REG {
                    return Err(MbFuncError::BadLoadAddress);
                }
                for (i, &reg) in regs.iter().enumerate() {
                    self.holding_regs[i + packet.addr as usize] = reg;
                }
                Ok(())
            },
            MbValue::MultiCoil(coils) => {
                if coils.len() + packet.addr as usize > Self::MAX_REG {
                    return Err(MbFuncError::BadLoadAddress);
                }
                for (i, &coil) in coils.iter().enumerate() {
                    self.coils[i + packet.addr as usize] = coil;
                }
                Ok(())
            },
            _ => Err(MbFuncError::BadPacketType)
        }

    }


    /// Obtain a clone of the Sender channel
    pub fn get_channel(&self) -> mpsc::Sender<MbMessage> {
        self.cmd_tx.clone()
    }

    /// Begin listening on the Receiver for messages, and process them.
    /// Each Cmd message received returns an appropriate Reply message.
    /// Continues to run until a Shutdown message is received.
    pub fn process(&mut self) {
        let sleep_time = Duration::from_millis(5); //TODO: make this configurable?
        loop {
            //check for a signal
            match self.cmd_rx.try_recv() {
                Ok(msg) => {
                    match msg.cmd {
                        MbMessageCmd::ReadHoldingRegs(extent) => {
                            if let Some(reply_tx) = msg.reply_tx {
                                match self.read_holding_regs(&extent) {
                                    Ok(msg) => self.send_reply(msg, &reply_tx),
                                    Err(MbFuncError::MbProtoExcep(e)) => self.send_exception(0x83, e, &reply_tx),
                                    Err(e) => self.send_internal_error(e, &reply_tx),
                                }
                            }
                        },
                        MbMessageCmd::ReadCoils(extent) => {
                            if let Some(reply_tx) = msg.reply_tx {
                                match self.read_coils(&extent) {
                                    Ok(msg) => self.send_reply(msg, &reply_tx),
                                    Err(MbFuncError::MbProtoExcep(e)) => self.send_exception(0x81, e, &reply_tx),
                                    Err(e) => self.send_internal_error(e, &reply_tx),
                                }
                            }
                        },
                        MbMessageCmd::WriteSingleHoldingReg(packet) => {
                            if let Some(reply_tx) = msg.reply_tx {
                                match self.write_single_holding_reg(packet) {
                                    Ok(msg) => self.send_reply(msg, &reply_tx),
                                    Err(MbFuncError::MbProtoExcep(e)) => self.send_exception(0x86, e, &reply_tx),
                                    Err(e) => self.send_internal_error(e, &reply_tx),
                                }
                            }
                        },
                        MbMessageCmd::WriteSingleCoil(packet) => {
                            if let Some(reply_tx) = msg.reply_tx {
                                match self.write_single_coil(packet) {
                                    Ok(msg) => self.send_reply(msg, &reply_tx),
                                    Err(MbFuncError::MbProtoExcep(e)) => self.send_exception(0x85, e, &reply_tx),
                                    Err(e) => self.send_internal_error(e, &reply_tx),
                                }
                            }
                        },
                        MbMessageCmd::WriteMultipleHoldingRegs(packet) => {
                            if let Some(reply_tx) = msg.reply_tx {
                                match self.write_multiple_holding_regs(packet) {
                                    Ok(msg) => self.send_reply(msg, &reply_tx),
                                    Err(MbFuncError::MbProtoExcep(e)) => self.send_exception(0x90, e, &reply_tx),
                                    Err(e) => self.send_internal_error(e, &reply_tx),
                                }
                            }
                        },

                        MbMessageCmd::WriteMultipleCoils(packet) => {
                            if let Some(reply_tx) = msg.reply_tx {
                            match self.write_multiple_coils(packet) {
                                    Ok(msg) => self.send_reply(msg, &reply_tx),
                                    Err(MbFuncError::MbProtoExcep(e)) => self.send_exception(0x8f, e, &reply_tx),
                                    Err(e) => self.send_internal_error(e, &reply_tx),
                                }
                            }
                        },

                        MbMessageCmd::Shutdown => break,
                    }
                },
                Err(mpsc::TryRecvError::Empty) => {
                    thread::sleep(sleep_time);
                },

                Err(mpsc::TryRecvError::Disconnected) => break
            }

        }
    }

    /// Handle the read_coils request.
    fn read_coils(&self, extent: &MbExtent) -> MbFuncResult {
        let addr: usize = extent.addr as usize;
        let quantity: usize = extent.quantity as usize;

        if addr > Self::MAX_COIL || addr > Self::MAX_COIL - quantity {
            return Err(MbFuncError::MbProtoExcep(ModbusProtoExcepCode::IllegalDataAddress));
        }
        if quantity == 0 || quantity > 0x07d0 {
            return Err(MbFuncError::MbProtoExcep(ModbusProtoExcepCode::IllegalDataValue));
        }
        let coils = Vec::from(&self.coils[addr .. (addr+quantity)]);

        let packet = MbPacket {
            addr: addr.try_into().unwrap(),
            value: MbValue::MultiCoil(coils),
        };
        Ok(MbMessageReply::ReadCoils(packet))
    }

    /// Handle the read_holding_regs request.
    fn read_holding_regs(&self, extent: &MbExtent) -> MbFuncResult {
        let addr: usize = extent.addr as usize;
        let quantity: usize = extent.quantity as usize;

        if addr > Self::MAX_REG || addr > Self::MAX_REG - quantity {
            return Err(MbFuncError::MbProtoExcep(ModbusProtoExcepCode::IllegalDataAddress));
        }
        if quantity == 0 || quantity > 0x07d {
            return Err(MbFuncError::MbProtoExcep(ModbusProtoExcepCode::IllegalDataValue));
        }
        let regs = Vec::from(&self.holding_regs[addr .. (addr+quantity)]);

        let packet = MbPacket {
            addr: addr.try_into().unwrap(),
            value: MbValue::MultiReg(regs),
        };
        Ok(MbMessageReply::ReadHoldingRegs(packet))
    }

    /// Handle the write_single_coil request.
    fn write_single_coil(&mut self, packet: MbPacket) -> MbFuncResult {
        let addr: usize = packet.addr as usize;
        if addr > Self::MAX_COIL {
            return Err(MbFuncError::MbProtoExcep(ModbusProtoExcepCode::IllegalDataAddress));
        }
        if let MbValue::SingleCoil(value) = packet.value {
            self.coils[addr] = value;
            Ok(MbMessageReply::WriteSingleCoil(packet))
        }
        else {
            Err(MbFuncError::BadPacketType)
        }
    }

    /// Handle the write_single_reg request.
    fn write_single_holding_reg(&mut self, packet: MbPacket) -> MbFuncResult {
        let addr = packet.addr as usize;
        if addr > Self::MAX_REG {
            return Err(MbFuncError::MbProtoExcep(ModbusProtoExcepCode::IllegalDataAddress));
        }
        if let MbValue::SingleReg(value) = packet.value {
            self.holding_regs[addr] = value;
            Ok(MbMessageReply::WriteSingleHoldingReg(packet))
        }
        else {
            Err(MbFuncError::BadPacketType)
        }
    }

    ///Handle the write_holding_regs request.
    fn write_multiple_holding_regs(&mut self, packet: MbPacket) -> MbFuncResult {
        let addr: usize = packet.addr as usize;

        if let MbValue::MultiReg(values) = packet.value {
            let quantity = values.len();
            if addr > Self::MAX_REG || addr > Self::MAX_REG - quantity {
                return Err(MbFuncError::MbProtoExcep(ModbusProtoExcepCode::IllegalDataAddress));
            }
            if quantity == 0 || quantity > 0x07b {
                return Err(MbFuncError::MbProtoExcep(ModbusProtoExcepCode::IllegalDataValue));
            }

            for (i, value) in values.iter().enumerate() {
                self.holding_regs[addr + i] = *value;
            }
            let extent = MbExtent {
                addr: packet.addr,
                quantity: quantity.try_into().unwrap(),
            };
            Ok(MbMessageReply::WriteMultipleHoldingRegs(extent))
        }
        else {
            Err(MbFuncError::BadPacketType)
        }
    }

    ///Handle the write_holding_regs request.
    fn write_multiple_coils(&mut self, packet: MbPacket) -> MbFuncResult {
        let addr: usize = packet.addr as usize;

        if let MbValue::MultiCoil(values) = packet.value {
            let quantity = values.len();
            if addr > Self::MAX_REG || addr > Self::MAX_REG - quantity {
                return Err(MbFuncError::MbProtoExcep(ModbusProtoExcepCode::IllegalDataAddress));
            }
            if quantity == 0 || quantity > 0x07b0 {
                return Err(MbFuncError::MbProtoExcep(ModbusProtoExcepCode::IllegalDataValue));
            }

            for (i, value) in values.iter().enumerate() {
                self.coils[addr + i] = *value;
            }
            let extent = MbExtent {
                addr: packet.addr,
                quantity: quantity.try_into().unwrap(),
            };
            Ok(MbMessageReply::WriteMultipleCoils(extent))
        }
        else {
            Err(MbFuncError::BadPacketType)
        }
    }

    fn send_reply(&self, reply: MbMessageReply, reply_tx: &mpsc::Sender<MbMessageReply>) {
        // we do not care if the other end has failed, so ignore the result
        let _ = reply_tx.send(reply);
    }


    fn send_exception(&self, error_code: u8, exception_code: ModbusProtoExcepCode, reply_tx: &mpsc::Sender<MbMessageReply>) {
        // we do not care if the other end has failed, so ignore the result
        let excep = ModbusException {
            error_code,
            exception_code,
        };
        let _ = reply_tx.send(MbMessageReply::ModbusException(excep));
    }

    fn send_internal_error(&self, error: MbFuncError, reply_tx: &mpsc::Sender<MbMessageReply>) {
        // we do not care if the other end has failed, so ignore the result
        let _ = reply_tx.send(MbMessageReply::InternalError(error));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Setup {
        pub tx: mpsc::Sender<MbMessageReply>,
        pub rx: mpsc::Receiver<MbMessageReply>,
        pub cmd_tx: mpsc::Sender<MbMessage>,
        pub handle: Option<thread::JoinHandle<()>>,
        pub timeout: Duration
    }
    impl Setup {
        pub fn shutdown(&mut self) {
            self.cmd_tx.send(MbMessage {
                cmd: MbMessageCmd::Shutdown,
                reply_tx: None
            }).unwrap();
            //don't call this twice lol
            self.handle.take().unwrap().join().unwrap();
        }
    }
    fn setup(load_coils: Vec<bool>, coil_addr: u16, load_regs: Vec<u16>, reg_addr: u16) -> Setup {
        let mut mbcore = MbCore::new();
        let coil_packet = MbPacket {
            addr: coil_addr,
            value: MbValue::MultiCoil(load_coils)
        }; 
        assert!(mbcore.load_bulk(coil_packet).is_ok());
        let reg_packet = MbPacket {
            addr: reg_addr,
            value: MbValue::MultiReg(load_regs)
        };
        assert!(mbcore.load_bulk(reg_packet).is_ok());
        let cmd_tx = mbcore.get_channel();
        let handle = Some(thread::spawn(move || mbcore.process()));
        let (tx, rx) = mpsc::channel();
        Setup {
            tx,
            rx,
            cmd_tx,
            handle,
            timeout: Duration::from_millis(200)
        }
    }

    #[test]
    fn read_coils() {
        let test_coils = vec![true, false, true, false];
        let test_addr: u16 = 0;
        let mut s = setup(test_coils.clone(), test_addr, vec![], 0);

        let extent = MbExtent { addr: test_addr, quantity: test_coils.len() as u16 };

        s.cmd_tx.send(MbMessage {
            cmd: MbMessageCmd::ReadCoils(extent),
            reply_tx: Some(s.tx.clone())
        }).unwrap();

        let reply = s.rx.recv_timeout(s.timeout).unwrap();
        if let MbMessageReply::ReadCoils(packet) = reply {
            if let MbValue::MultiCoil(reply_coils) = packet.value {
                assert_eq!(packet.addr, test_addr);
                assert_eq!(reply_coils.len(), test_coils.len());
                assert_eq!(test_coils, reply_coils);
            }
            else {
                panic!("Wrong packet value type received.");
            }
        }
        else {
            panic!("Wrong packet type received.");
        }

        s.shutdown();
    }

    #[test]
    fn write_single_coil() {
        let test_coils = vec![true, true, true, true];
        let mut s = setup(test_coils.clone(), 0, vec![], 0);
        let test_addr = 1;
        let test_value = !test_coils[test_addr as usize];


        let packet = MbPacket {
            addr: test_addr,
            value: MbValue::SingleCoil(test_value)
        };
        s.cmd_tx.send(MbMessage {
            cmd: MbMessageCmd::WriteSingleCoil(packet),
            reply_tx: Some(s.tx.clone())
        }).unwrap();

        let reply = s.rx.recv_timeout(s.timeout).unwrap();
        if let MbMessageReply::WriteSingleCoil(packet) = reply {
            if let MbValue::SingleCoil(reply_coil) = packet.value {
                assert_eq!(packet.addr, test_addr);
                assert_eq!(reply_coil, test_value);
            }
            else {
                panic!("Wrong packet value type received.");
            }
        }
        else {
            panic!("Wrong packet type received.");
        }

        s.shutdown();
    }

    #[test]
    fn read_holding_regs() {
        let test_regs: Vec<u16> = vec![0x01, 0x02, 0x03, 0x04];
        let test_addr: u16 = 1;
        let mut s = setup(vec![], 0, test_regs.clone(), test_addr);

        let extent = MbExtent {
            addr: test_addr,
            quantity: test_regs.len() as u16
        };
        s.cmd_tx.send(MbMessage {
            cmd: MbMessageCmd::ReadHoldingRegs(extent),
            reply_tx: Some(s.tx.clone())
        }).unwrap();

        let reply = s.rx.recv_timeout(s.timeout).unwrap();
        if let MbMessageReply::ReadHoldingRegs(packet) = reply {
            if let MbValue::MultiReg(reply_regs) = packet.value {
                assert_eq!(reply_regs.len(), test_regs.len());
                assert_eq!(reply_regs, test_regs);
            }
            else {
                panic!("Wrong packet value type received.");
            }
        }
        else {
            panic!("Wrong packet type received.");
        }

        s.shutdown();
    }

    #[test]
    fn write_single_holding_reg() {
        let test_regs: Vec<u16> = vec![0x01, 0x02, 0x03, 0x04];
        let test_addr = 2;
        let mut s = setup(vec![], 0, test_regs.clone(), 0);

        let test_value = test_regs[test_addr] + 5;
        let packet = MbPacket {
            addr: test_addr as u16,
            value: MbValue::SingleReg(test_value)
        };
        s.cmd_tx.send(MbMessage {
            cmd: MbMessageCmd::WriteSingleHoldingReg(packet),
            reply_tx: Some(s.tx.clone())
        }).unwrap();

        let reply = s.rx.recv_timeout(s.timeout).unwrap();
        if let MbMessageReply::WriteSingleHoldingReg(packet) = reply {
            if let MbValue::SingleReg(reply_reg) = packet.value {
                assert_eq!(packet.addr, test_addr as u16);
                assert_eq!(reply_reg, test_value);
            }
            else {
                panic!("Wrong packet value type received.");
            }
        }
        else {
            panic!("Wrong packet type received.");
        }

        s.shutdown();
    }

    #[test]
    fn write_multiple_holding_regs() {
        let test_regs: Vec<u16> = vec![0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
        let test_addr = 2;
        let mut s = setup(vec![], 0, test_regs.clone(), 0);

        let packet = MbPacket {
            addr: test_addr as u16,
            value: MbValue::MultiReg(test_regs.clone())
        };
        s.cmd_tx.send(MbMessage {
            cmd: MbMessageCmd::WriteMultipleHoldingRegs(packet),
            reply_tx: Some(s.tx.clone())
        }).unwrap();

        let reply = s.rx.recv_timeout(s.timeout).unwrap();
        if let MbMessageReply::WriteMultipleHoldingRegs(extent) = reply {
                assert_eq!(extent.addr, test_addr as u16);
                assert_eq!(extent.quantity, test_regs.len() as u16);
        }
        else {
            panic!("Wrong packet type received.");
        }

        s.shutdown();
    }

    #[test]
    fn write_multiple_coils() {
        let test_coils = vec![true, true, true, true];
        let test_addr = 2;
        let mut s = setup(test_coils.clone(), test_addr, vec![], 0);


        let packet = MbPacket {
            addr: test_addr,
            value: MbValue::MultiCoil(test_coils.clone())
        };
        s.cmd_tx.send(MbMessage {
            cmd: MbMessageCmd::WriteMultipleCoils(packet),
            reply_tx: Some(s.tx.clone())
        }).unwrap();

        let reply = s.rx.recv_timeout(s.timeout).unwrap();
        if let MbMessageReply::WriteMultipleCoils(extent) = reply {
            assert_eq!(extent.addr, test_addr);
            assert_eq!(extent.quantity, test_coils.len() as u16);
        }
        else {
            panic!("Wrong packet type received.");
        }

        s.shutdown();
    }

    #[test]
    fn test_errors() {
        let test_regs: Vec<u16> = vec![0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
        let test_coils = vec![true, true, true, true, true, true, true, true];
        let mut s = setup(vec![], 0, vec![], 0);

        let test_addr = MbCore::MAX_REG;

        // bad address, write_multiple_holding_regs
        let packet = MbPacket {
            addr: test_addr as u16,
            value: MbValue::MultiReg(test_regs.clone())
        };
        s.cmd_tx.send(MbMessage {
            cmd: MbMessageCmd::WriteMultipleHoldingRegs(packet),
            reply_tx: Some(s.tx.clone())
        }).unwrap();

        let reply = s.rx.recv_timeout(s.timeout).unwrap();
        if let MbMessageReply::ModbusException(excep) = reply {
            assert_eq!(excep.exception_code, ModbusProtoExcepCode::IllegalDataAddress);
            //TODO: check error code
        }
        else {
            panic!("Wrong packet type received.");
        }

        // bad address, write_multiple_coils
        let packet = MbPacket {
            addr: test_addr as u16,
            value: MbValue::MultiCoil(test_coils.clone())
        };
        s.cmd_tx.send(MbMessage {
            cmd: MbMessageCmd::WriteMultipleCoils(packet),
            reply_tx: Some(s.tx.clone())
        }).unwrap();

        let reply = s.rx.recv_timeout(s.timeout).unwrap();
        if let MbMessageReply::ModbusException(excep) = reply {
            assert_eq!(excep.exception_code, ModbusProtoExcepCode::IllegalDataAddress);
            //TODO: check error code
        }
        else {
            panic!("Wrong packet type received.");
        }


        // mismatched packet
        let packet = MbPacket {
            addr: test_addr as u16,
            value: MbValue::MultiCoil(test_coils.clone())
        };
        s.cmd_tx.send(MbMessage {
            cmd: MbMessageCmd::WriteSingleHoldingReg(packet),
            reply_tx: Some(s.tx.clone())
        }).unwrap();

        let reply = s.rx.recv_timeout(s.timeout).unwrap();
        if let MbMessageReply::InternalError(error) = reply {
            assert_eq!(error, MbFuncError::BadPacketType);
        }
        else {
            panic!("Wrong packet type received.");
        }

        s.shutdown();
    }

    #[test]
    fn echo_test_coil() {
        let test_coils = vec![true, false, true, true, false, true, false, true];
        let test_addr = 0;
        let mut s = setup(vec![], 0, vec![], 0);


        // write_multiple_coils
        let packet = MbPacket {
            addr: test_addr,
            value: MbValue::MultiCoil(test_coils.clone())
        };
        s.cmd_tx.send(MbMessage {
            cmd: MbMessageCmd::WriteMultipleCoils(packet),
            reply_tx: Some(s.tx.clone())
        }).unwrap();

        let reply = s.rx.recv_timeout(s.timeout).unwrap();
        if let MbMessageReply::WriteMultipleCoils(extent) = reply {
            assert_eq!(extent.addr, test_addr);
            assert_eq!(extent.quantity, test_coils.len() as u16);
        }
        else {
            panic!("Wrong packet type received.");
        }

        let extent = MbExtent {
            addr: test_addr,
            quantity: test_coils.len() as u16
        };
        s.cmd_tx.send(MbMessage {
            cmd: MbMessageCmd::ReadCoils(extent),
            reply_tx: Some(s.tx.clone())
        }).unwrap();

        let reply = s.rx.recv_timeout(s.timeout).unwrap();
        if let MbMessageReply::ReadCoils(packet) = reply {
            if let MbValue::MultiCoil(reply_coils) = packet.value {
                assert_eq!(reply_coils, test_coils);
            }
            else {
                panic!("Wrong packet value type received.");
            }
        }
        else {
            panic!("Wrong packet type received.");
        }

        s.shutdown();
    }

    #[test]
    fn echo_test_reg() {
        let test_regs: Vec<u16> = vec![0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
        let test_addr = 0;
        let mut s = setup(vec![], 0, vec![], 0);


        // write_multiple_coils
        let packet = MbPacket {
            addr: test_addr,
            value: MbValue::MultiReg(test_regs.clone())
        };
        s.cmd_tx.send(MbMessage {
            cmd: MbMessageCmd::WriteMultipleHoldingRegs(packet),
            reply_tx: Some(s.tx.clone())
        }).unwrap();

        let reply = s.rx.recv_timeout(s.timeout).unwrap();
        if let MbMessageReply::WriteMultipleHoldingRegs(extent) = reply {
            assert_eq!(extent.addr, test_addr);
            assert_eq!(extent.quantity, test_regs.len() as u16);
        }
        else {
            panic!("Wrong packet type received.");
        }

        let extent = MbExtent {
            addr: test_addr,
            quantity: test_regs.len() as u16
        };
        s.cmd_tx.send(MbMessage {
            cmd: MbMessageCmd::ReadHoldingRegs(extent),
            reply_tx: Some(s.tx.clone())
        }).unwrap();

        let reply = s.rx.recv_timeout(s.timeout).unwrap();
        if let MbMessageReply::ReadHoldingRegs(packet) = reply {
            if let MbValue::MultiReg(reply_regs) = packet.value {
                assert_eq!(reply_regs, test_regs);
            }
            else {
                panic!("Wrong packet value type received.");
            }
        }
        else {
            panic!("Wrong packet type received.");
        }

        s.shutdown();
    }

    //TODO: echo tests and repeated writes
}
