//! Linux ALSA backend — UMP stub until kernel `/dev/snd/ump*` is wired.
//!
//! No `alsa` / `alsa-sys` dependency: hosts can build without `libasound`.
//! See `docs/ump-backend-compliance.md`.

use crate::errors::*;
use crate::Ignore;

#[derive(Debug)]
pub struct MidiInput {
    ignore_flags: Ignore,
}

#[derive(Clone, PartialEq)]
pub struct MidiInputPort {
    id: String,
}

impl MidiInputPort {
    pub fn id(&self) -> String {
        self.id.clone()
    }
}

pub struct MidiInputConnection<T> {
    ignore_flags: Ignore,
    user_data: Option<T>,
}

impl MidiInput {
    pub fn new(_client_name: &str) -> Result<Self, InitError> {
        Ok(MidiInput {
            ignore_flags: Ignore::None,
        })
    }

    pub fn ignore(&mut self, flags: Ignore) {
        self.ignore_flags = flags;
    }

    pub(crate) fn ports_internal(&self) -> Vec<crate::common::MidiInputPort> {
        Vec::new()
    }

    pub fn port_count(&self) -> usize {
        0
    }

    pub fn port_name(&self, _port: &MidiInputPort) -> Result<String, PortInfoError> {
        Err(PortInfoError::InvalidPort)
    }

    pub fn connect<F, T: Send>(
        self,
        port: &MidiInputPort,
        _port_name: &str,
        _callback: F,
        _data: T,
    ) -> Result<MidiInputConnection<T>, ConnectError<MidiInput>>
    where
        F: FnMut(u64, &[u32], &mut T) + Send + 'static,
    {
        let _ = port;
        Err(ConnectError::other(
            "UMP MIDI 2.0 not implemented on this backend yet (see docs/ump-backend-compliance.md)",
            self,
        ))
    }
}

impl<T> MidiInputConnection<T> {
    pub fn close(mut self) -> (MidiInput, T) {
        (
            MidiInput {
                ignore_flags: self.ignore_flags,
            },
            self.user_data.take().unwrap(),
        )
    }
}

#[derive(Debug)]
pub struct MidiOutput;

#[derive(Clone, PartialEq)]
pub struct MidiOutputPort {
    id: String,
}

impl MidiOutputPort {
    pub fn id(&self) -> String {
        self.id.clone()
    }
}

pub struct MidiOutputConnection;

impl MidiOutput {
    pub fn new(_client_name: &str) -> Result<Self, InitError> {
        Ok(MidiOutput)
    }

    pub(crate) fn ports_internal(&self) -> Vec<crate::common::MidiOutputPort> {
        Vec::new()
    }

    pub fn port_count(&self) -> usize {
        0
    }

    pub fn port_name(&self, _port: &MidiOutputPort) -> Result<String, PortInfoError> {
        Err(PortInfoError::InvalidPort)
    }

    pub fn connect(
        self,
        port: &MidiOutputPort,
        _port_name: &str,
    ) -> Result<MidiOutputConnection, ConnectError<MidiOutput>> {
        let _ = port;
        Err(ConnectError::other(
            "UMP MIDI 2.0 not implemented on this backend yet (see docs/ump-backend-compliance.md)",
            self,
        ))
    }
}

impl MidiOutputConnection {
    pub fn close(self) -> MidiOutput {
        MidiOutput
    }

    pub fn send(&mut self, words: &[u32]) -> Result<(), SendError> {
        let _ = words;
        Err(SendError::Other(
            "UMP MIDI 2.0 not implemented on this backend yet (see docs/ump-backend-compliance.md)",
        ))
    }
}
