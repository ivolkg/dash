//! CAN bus and OBD-II layer for dash.
//!
//! Migration steps 2 and 3 in `docs/OXIDIZING.md`: pure OBD decoders and the
//! ELM327 parser land here first, then the SocketCAN and ELM327 transports.

/// OBD-II mode 01 PIDs that dash queries today (`src/obd/command.cpp`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Pid {
    EngineLoad = 0x04,
    CoolantTemp = 0x05,
    Rpm = 0x0C,
    Speed = 0x0D,
    IntakeTemp = 0x0F,
    Maf = 0x10,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pid_values_match_obd_spec() {
        assert_eq!(Pid::Rpm as u8, 0x0C);
        assert_eq!(Pid::Speed as u8, 0x0D);
    }
}
