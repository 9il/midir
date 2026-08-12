//! CoreMIDI backend — USB MIDI 2.0 / UMP via `MIDIEventList` (protocol 2.0).
//!
//! Shared by macOS and iOS. Does **not** use legacy `MIDIPacketList` MIDI 1.0 packing.

use std::sync::{Arc, Mutex};

use coremidi::{
    Client, Destination, Destinations, EventBuffer, EventList, InputPortWithContext, OutputPort,
    Protocol, Source, Sources, VirtualDestination, VirtualSource,
};

use crate::errors::*;
use crate::Ignore;

mod external {
    #[link(name = "CoreAudio", kind = "framework")]
    extern "C" {
        pub fn AudioConvertHostTimeToNanos(inHostTime: u64) -> u64;
        pub fn AudioGetCurrentHostTime() -> u64;
    }
}

pub struct MidiInput {
    client: Client,
    ignore_flags: Ignore,
}

#[derive(Clone)]
pub struct MidiInputPort {
    source: Arc<Source>,
}

impl MidiInputPort {
    pub fn id(&self) -> String {
        self.source.unique_id().unwrap_or(0).to_string()
    }
}

impl PartialEq for MidiInputPort {
    fn eq(&self, other: &Self) -> bool {
        match (self.source.unique_id(), other.source.unique_id()) {
            (Some(id1), Some(id2)) => id1 == id2,
            _ => false,
        }
    }
}

impl MidiInput {
    pub fn new(client_name: &str) -> Result<Self, InitError> {
        Client::new(client_name)
            .map(|client| MidiInput {
                client,
                ignore_flags: Ignore::None,
            })
            .map_err(|_| InitError)
    }

    pub(crate) fn ports_internal(&self) -> Vec<crate::common::MidiInputPort> {
        Sources
            .into_iter()
            .map(|s| crate::common::MidiInputPort {
                imp: MidiInputPort {
                    source: Arc::new(s),
                },
            })
            .collect()
    }

    pub fn ignore(&mut self, flags: Ignore) {
        // UMP path: flags are retained for API compatibility but not applied
        // (no MIDI-1 SysEx7 segmentation). Pass-through all UMPs.
        self.ignore_flags = flags;
    }

    pub fn port_count(&self) -> usize {
        Sources::count()
    }

    pub fn port_name(&self, port: &MidiInputPort) -> Result<String, PortInfoError> {
        port.source
            .display_name()
            .or_else(|| port.source.name())
            .ok_or(PortInfoError::CannotRetrievePortName)
    }

    pub fn connect<F, T: Send + 'static>(
        self,
        port: &MidiInputPort,
        port_name: &str,
        callback: F,
        data: T,
    ) -> Result<MidiInputConnection<T>, ConnectError<MidiInput>>
    where
        F: FnMut(u64, &[u32], &mut T) + Send + 'static,
    {
        let handler_data = Arc::new(Mutex::new(HandlerData {
            ignore_flags: self.ignore_flags,
            callback: Box::new(callback),
            user_data: Some(data),
        }));
        let handler_data2 = handler_data.clone();
        let mut iport = match self.client.input_port_with_protocol(
            port_name,
            Protocol::Midi20,
            move |event_list: &EventList, ctx: &mut ()| {
                let _ = ctx;
                let mut guard = handler_data2.lock().unwrap();
                deliver_event_list(event_list, &mut *guard);
            },
        ) {
            Ok(p) => p,
            Err(_) => return Err(ConnectError::other("error creating MIDI 2.0 input port", self)),
        };
        if iport.connect_source(&port.source, ()).is_err() {
            return Err(ConnectError::other(
                "error connecting MIDI input port",
                self,
            ));
        }
        Ok(MidiInputConnection {
            client: self.client,
            details: InputConnectionDetails::Explicit(iport, port.source.clone()),
            handler_data,
        })
    }

    pub fn create_virtual<F, T: Send + 'static>(
        self,
        port_name: &str,
        callback: F,
        data: T,
    ) -> Result<MidiInputConnection<T>, ConnectError<MidiInput>>
    where
        F: FnMut(u64, &[u32], &mut T) + Send + 'static,
    {
        let handler_data = Arc::new(Mutex::new(HandlerData {
            ignore_flags: self.ignore_flags,
            callback: Box::new(callback),
            user_data: Some(data),
        }));
        let handler_data2 = handler_data.clone();
        let vrt = match self.client.virtual_destination_with_protocol(
            port_name,
            Protocol::Midi20,
            move |event_list: &EventList| {
                let mut guard = handler_data2.lock().unwrap();
                deliver_event_list(event_list, &mut *guard);
            },
        ) {
            Ok(p) => p,
            Err(_) => {
                return Err(ConnectError::other(
                    "error creating virtual MIDI 2.0 destination",
                    self,
                ))
            }
        };
        Ok(MidiInputConnection {
            client: self.client,
            details: InputConnectionDetails::Virtual(vrt),
            handler_data,
        })
    }
}

fn deliver_event_list<T>(event_list: &EventList, handler: &mut HandlerData<T>) {
    let data = handler.user_data.as_mut().unwrap();
    for packet in event_list.iter() {
        let words = packet.data();
        if words.is_empty() {
            continue;
        }
        let mut timestamp = packet.timestamp();
        if cfg!(not(target_os = "ios")) && timestamp == 0 {
            timestamp = unsafe { external::AudioGetCurrentHostTime() };
        }
        let micros = if cfg!(target_os = "ios") {
            timestamp
        } else {
            (unsafe { external::AudioConvertHostTimeToNanos(timestamp) }) / 1000
        };
        (handler.callback)(micros, words, data);
    }
}

enum InputConnectionDetails {
    Explicit(InputPortWithContext<()>, Arc<Source>),
    #[allow(dead_code)]
    Virtual(VirtualDestination),
}

pub struct MidiInputConnection<T> {
    client: Client,
    #[allow(dead_code)]
    details: InputConnectionDetails,
    handler_data: Arc<Mutex<HandlerData<T>>>,
}

impl<T> MidiInputConnection<T> {
    pub fn close(mut self) -> (MidiInput, T) {
        if let InputConnectionDetails::Explicit(ref mut port, ref source) = self.details {
            let _ = port.disconnect_source(source);
        }
        let mut handler_data_locked = self.handler_data.lock().unwrap();
        (
            MidiInput {
                client: self.client,
                ignore_flags: handler_data_locked.ignore_flags,
            },
            handler_data_locked.user_data.take().unwrap(),
        )
    }
}

struct HandlerData<T> {
    ignore_flags: Ignore,
    callback: Box<dyn FnMut(u64, &[u32], &mut T) + Send>,
    user_data: Option<T>,
}

pub struct MidiOutput {
    client: Client,
}

#[derive(Clone)]
pub struct MidiOutputPort {
    dest: Arc<Destination>,
}

impl MidiOutputPort {
    pub fn id(&self) -> String {
        self.dest.unique_id().unwrap_or(0).to_string()
    }
}

impl PartialEq for MidiOutputPort {
    fn eq(&self, other: &Self) -> bool {
        match (self.dest.unique_id(), other.dest.unique_id()) {
            (Some(id1), Some(id2)) => id1 == id2,
            _ => false,
        }
    }
}

impl MidiOutput {
    pub fn new(client_name: &str) -> Result<Self, InitError> {
        Client::new(client_name)
            .map(|client| MidiOutput { client })
            .map_err(|_| InitError)
    }

    pub(crate) fn ports_internal(&self) -> Vec<crate::common::MidiOutputPort> {
        Destinations
            .into_iter()
            .map(|d| crate::common::MidiOutputPort {
                imp: MidiOutputPort { dest: Arc::new(d) },
            })
            .collect()
    }

    pub fn port_count(&self) -> usize {
        Destinations::count()
    }

    pub fn port_name(&self, port: &MidiOutputPort) -> Result<String, PortInfoError> {
        port.dest
            .display_name()
            .or_else(|| port.dest.name())
            .ok_or(PortInfoError::CannotRetrievePortName)
    }

    pub fn connect(
        self,
        port: &MidiOutputPort,
        port_name: &str,
    ) -> Result<MidiOutputConnection, ConnectError<MidiOutput>> {
        let oport = match self.client.output_port(port_name) {
            Ok(p) => p,
            Err(_) => return Err(ConnectError::other("error creating MIDI output port", self)),
        };
        Ok(MidiOutputConnection {
            client: self.client,
            details: OutputConnectionDetails::Explicit(oport, port.dest.clone()),
        })
    }

    pub fn create_virtual(
        self,
        port_name: &str,
    ) -> Result<MidiOutputConnection, ConnectError<MidiOutput>> {
        // Virtual sources still use MIDISourceCreate; UMP received path needs EventList.
        let vrt = match self.client.virtual_source(port_name) {
            Ok(p) => p,
            Err(_) => {
                return Err(ConnectError::other(
                    "error creating virtual MIDI source",
                    self,
                ))
            }
        };
        Ok(MidiOutputConnection {
            client: self.client,
            details: OutputConnectionDetails::Virtual(vrt),
        })
    }
}

enum OutputConnectionDetails {
    Explicit(OutputPort, Arc<Destination>),
    Virtual(VirtualSource),
}

pub struct MidiOutputConnection {
    client: Client,
    details: OutputConnectionDetails,
}

impl MidiOutputConnection {
    pub fn close(self) -> MidiOutput {
        MidiOutput {
            client: self.client,
        }
    }

    /// Send one or more Universal MIDI Packets as native-endian 32-bit words.
    pub fn send(&mut self, words: &[u32]) -> Result<(), SendError> {
        if words.is_empty() {
            return Ok(());
        }
        let send_time =
            if cfg!(feature = "coremidi_send_timestamped") && cfg!(not(target_os = "ios")) {
                unsafe { external::AudioGetCurrentHostTime() }
            } else {
                0
            };
        let mut buf = EventBuffer::with_capacity(64 + words.len() * 4, Protocol::Midi20);
        buf.push(send_time, words);
        match self.details {
            OutputConnectionDetails::Explicit(ref port, ref dest) => port
                .send(dest, &buf)
                .map_err(|_| SendError::Other("error sending UMP EventList to port")),
            OutputConnectionDetails::Virtual(ref vrt) => {
                // VirtualSource::received is PacketList-based in coremidi 0.9; use EventList API if available.
                // Fall back: MIDIReceivedEventList via send path — coremidi VirtualSource may only take PacketList.
                // Reject virtual UMP send until coremidi exposes ReceivedEventList.
                let _ = vrt;
                Err(SendError::Other(
                    "virtual MIDI source UMP send not supported yet; use a physical destination",
                ))
            }
        }
    }
}
