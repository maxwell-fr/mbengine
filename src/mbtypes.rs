//! Contains various helper types.
//!
use std::sync::mpsc;

/// Represents an address (coil or register)
pub type MbAddress = u16;

/// Represents a quantity of coils or registers
pub type MbQuantity = u16;

/// Represents a quantity of bytes
pub type MbByteCount = u16;

/// Represents a Modbus value, one of single or multiple coils or registers
#[derive(Debug, Clone, PartialEq)]
pub enum MbValue {
    /// A single coil (bit)
    SingleCoil(bool),
    /// A Vec of coils (bits)
    MultiCoil(Vec<bool>),
    /// A single register
    SingleReg(u16),
    /// A Vec of registers
    MultiReg(Vec<u16>)
}

/// Represents a range of registers or coils, starting at addr.
/// Does not contain the values themselves.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MbExtent {
    /// Address of the start of the extent
    pub addr: MbAddress,
    /// The number of registers or coils in the extent
    pub quantity: MbQuantity
}

/// Represents a value, coming from or destined for addr.
#[derive(Debug, Clone, PartialEq)]
pub struct MbPacket {
    /// Address of the register or coil: i.e., where the value
    /// is coming from or going to.
    pub addr: MbAddress,
    /// Value of the register or coil
    pub value: MbValue
}

/// Messages that may be sent over the command channel
pub enum MbMessageCmd {
    /// Read holding registers (Modbus function 0x03)
    ReadHoldingRegs(MbExtent),
    /// Read coils (Modbus function 0x01)
    ReadCoils(MbExtent),
    /// Write a single holding register (Modbus function 0x06)
    WriteSingleHoldingReg(MbPacket),
    /// Write a single coil (Modbus function 0x05)
    WriteSingleCoil(MbPacket),
    /// Write a block of holding registers (Modbus function 0x10)
    WriteMultipleHoldingRegs(MbPacket),
    /// Write a block of coils (Modbus function 0x0f)
    WriteMultipleCoils(MbPacket),
    /// Request shutdown of the Modbus Process
    Shutdown
}

/// Messages that may be sent over the reply channel
#[derive(Debug, PartialEq)]
pub enum MbMessageReply {
    /// The functionality requested is not implemented
    NotImplemented,
    /// An internal error occurred
    InternalError(MbFuncError),
    /// A Reply to a ReadCoils cmd
    ReadCoils(MbPacket),
    /// A Reply to a WriteSingleCoil cmd
    WriteSingleCoil(MbPacket),
    /// A Reply to a WriteSingleHoldingReg cmd
    WriteSingleHoldingReg(MbPacket),
    /// A reply to a ReadHoldingRegs cmd
    ReadHoldingRegs(MbPacket),
    /// A reply to a WriteMultipleHoldingRegs cmd
    WriteMultipleHoldingRegs(MbExtent),
    /// A reply to a WriteMultipleCoils cmd
    WriteMultipleCoils(MbExtent),
    /// A Modbus exception occurred
    ModbusException(ModbusException)
}

/// A message with an optional return channel
pub struct MbMessage {
    /// The command component of the message
    pub cmd: MbMessageCmd,
    /// The return mpsc channel for any replies
    pub reply_tx: Option<mpsc::Sender<MbMessageReply>>
}

/// Represents possible Modbus protocol exception codes, following the Modbus specification
#[derive(Debug, Clone, PartialEq)]
pub enum ModbusProtoExcepCode {
    /// Illegal function call (0x01)
    IllegalFunction = 0x01,
    /// Illegal data address (0x02)
    IllegalDataAddress = 0x02,
    /// Illegal data value (0x03)
    IllegalDataValue = 0x03,
    /// Server device failure (0x04)
    ServerDeviceFailure = 0x04,
    /// Acknowledge (0x05)
    Acknowledge = 0x05,
    /// Server device busy (0x06)
    ServerDeviceBusy = 0x06,
    /// Memory Parity Error (0x08)
    MemoryParityError = 0x08,
    /// Gateway path unavailable (0x0a)
    GatewayPathUnavailable = 0x0a,
    /// Gateway target device not responding (0x0b)
    GatewayTargetDeviceNotResponding = 0x0b
}

/// Represents a Modbus error and exception 
#[derive(Debug, Clone, PartialEq)]
pub struct ModbusException {
    /// The error code component of an error. Usually this is (function_code & 0x80)
    pub error_code: u8, // TODO: make this an enum?
    /// The exception code component of an error
    pub exception_code: ModbusProtoExcepCode
}

/// A wrapper for MbCore Err() values
#[derive(Debug, Clone, PartialEq)]
pub enum MbFuncError {
    /// A Modbus exception occurred
    MbProtoExcep(ModbusProtoExcepCode),
    /// An error occurred trying to transmit on the mpsc channel
    TransmitError,
    /// An invalid or out of range address was provided to a bulk load function
    BadLoadAddress,
    /// A packet type arrived that is malformed, e.g., the value does not match the request type
    BadPacketType,
}

/// Convenience type for MbCore function results
pub type MbFuncResult = Result<MbMessageReply, MbFuncError>;
