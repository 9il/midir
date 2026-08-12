use parking_lot::ReentrantMutex as Mutex;
use std::alloc::{alloc, dealloc, Layout};
use std::ffi::{c_void, OsString};
use std::mem::MaybeUninit;
use std::os::windows::ffi::OsStringExt;
use std::ptr::null_mut;
use std::thread::sleep;
use std::time::Duration;
use std::{mem, ptr, slice};

use log::warn;

use windows::core::PSTR;
use windows::Win32::Media::Audio::{
    midiInAddBuffer, midiInClose, midiInGetDevCapsW, midiInGetNumDevs, midiInMessage, midiInOpen,
    midiInPrepareHeader, midiInReset, midiInStart, midiInStop, midiInUnprepareHeader, midiOutClose,
    midiOutGetDevCapsW, midiOutGetNumDevs, midiOutLongMsg, midiOutMessage, midiOutOpen,
    midiOutPrepareHeader, midiOutReset, midiOutShortMsg, midiOutUnprepareHeader, CALLBACK_FUNCTION,
    CALLBACK_NULL, HMIDIIN, HMIDIOUT, MIDIERR_NOTREADY, MIDIERR_STILLPLAYING, MIDIHDR, MIDIINCAPSW,
    MIDIOUTCAPSW,
};
use windows::Win32::Media::Multimedia::{DRV_QUERYDEVICEINTERFACE, DRV_QUERYDEVICEINTERFACESIZE};
use windows::Win32::Media::{MMSYSERR_ALLOCATED, MMSYSERR_BADDEVICEID, MMSYSERR_NOERROR};

#[allow(non_camel_case_types)]
#[allow(clippy::upper_case_acronyms)]
type ULONG = u32;
#[allow(non_camel_case_types)]
#[allow(clippy::upper_case_acronyms)]
type UINT = u32;
#[allow(non_camel_case_types)]
#[allow(clippy::upper_case_acronyms)]
type DWORD = u32;
#[allow(non_camel_case_types)]
#[allow(clippy::upper_case_acronyms)]
type DWORD_PTR = usize;

use crate::errors::*;
use crate::{Ignore, MidiMessage};

mod handler;

const MIDIR_SYSEX_BUFFER_SIZE: usize = 1024;
const MIDIR_SYSEX_BUFFER_COUNT: usize = 4;

// helper for string conversion
fn from_wide_ptr(ptr: *const u16, max_len: usize) -> OsString {
    unsafe {
        assert!(!ptr.is_null());
        let len = (0..max_len as isize)
            .position(|i| *ptr.offset(i) == 0)
            .unwrap();
        let slice = slice::from_raw_parts(ptr, len);
        OsString::from_wide(slice)
    }
}

#[derive(Debug)]
pub struct MidiInput {
    ignore_flags: Ignore,
}

#[derive(Clone)]
pub struct MidiInputPort {
    name: String,
    interface_id: Box<[u16]>,
}

impl MidiInputPort {
    pub fn id(&self) -> String {
        String::from_utf16_lossy(&self.interface_id[..self.interface_id.len() - 1])
    }
}

impl PartialEq for MidiInputPort {
    fn eq(&self, other: &Self) -> bool {
        self.interface_id == other.interface_id
    }
}

pub struct MidiInputConnection<T> {
    handler_data: Box<HandlerData<T>>,
}

impl MidiInputPort {
    fn count() -> UINT {
        unsafe { midiInGetNumDevs() }
    }

    /// Queries the interface ID. The result contains a terminating zero.
    fn interface_id(port_number: UINT) -> Result<Box<[u16]>, PortInfoError> {
        let mut buffer_size: ULONG = 0;
        let result = unsafe {
            midiInMessage(
                Some(HMIDIIN(port_number as *mut c_void)),
                DRV_QUERYDEVICEINTERFACESIZE,
                Some(&mut buffer_size as *mut _ as DWORD_PTR),
                None,
            )
        };
        if result == MMSYSERR_BADDEVICEID {
            return Err(PortInfoError::PortNumberOutOfRange);
        } else if result != MMSYSERR_NOERROR {
            return Err(PortInfoError::CannotRetrievePortName);
        }
        let mut buffer = Vec::<u16>::with_capacity(buffer_size as usize / 2);
        unsafe {
            let result = midiInMessage(
                Some(HMIDIIN(port_number as *mut c_void)),
                DRV_QUERYDEVICEINTERFACE,
                Some(buffer.as_mut_ptr() as usize),
                Some(buffer_size as DWORD_PTR),
            );
            if result == MMSYSERR_BADDEVICEID {
                return Err(PortInfoError::PortNumberOutOfRange);
            } else if result != MMSYSERR_NOERROR {
                return Err(PortInfoError::CannotRetrievePortName);
            }
            buffer.set_len(buffer_size as usize / 2);
        }
        //println!("{}", from_wide_ptr(buffer.as_ptr(), buffer.len()).to_string_lossy().into_owned());
        assert!(buffer.last().cloned() == Some(0));
        Ok(buffer.into_boxed_slice())
    }

    fn name(port_number: UINT) -> Result<String, PortInfoError> {
        let mut device_caps: MaybeUninit<MIDIINCAPSW> = MaybeUninit::uninit();
        let result = unsafe {
            midiInGetDevCapsW(
                port_number as usize,
                device_caps.as_mut_ptr(),
                mem::size_of::<MIDIINCAPSW>() as u32,
            )
        };
        if result == MMSYSERR_BADDEVICEID {
            return Err(PortInfoError::PortNumberOutOfRange);
        } else if result != MMSYSERR_NOERROR {
            return Err(PortInfoError::CannotRetrievePortName);
        }
        let device_caps = unsafe { device_caps.assume_init() };
        let pname_ptr: *const [u16; 32] = std::ptr::addr_of!(device_caps.szPname);
        let output = from_wide_ptr(pname_ptr as *const _, 32)
            .to_string_lossy()
            .into_owned();
        Ok(output)
    }

    fn from_port_number(port_number: UINT) -> Result<Self, PortInfoError> {
        Ok(MidiInputPort {
            name: Self::name(port_number)?,
            interface_id: Self::interface_id(port_number)?,
        })
    }

    fn current_port_number(&self) -> Option<UINT> {
        for i in 0..Self::count() {
            if let Ok(name) = Self::name(i) {
                if name != self.name {
                    continue;
                }
                if let Ok(id) = Self::interface_id(i) {
                    if id == self.interface_id {
                        return Some(i);
                    }
                }
            }
        }
        None
    }
}

struct SysexBuffer([*mut MIDIHDR; MIDIR_SYSEX_BUFFER_COUNT]);
unsafe impl Send for SysexBuffer {}

struct MidiInHandle(Mutex<HMIDIIN>);
unsafe impl Send for MidiInHandle {}

/// This is all the data that is stored on the heap as long as a connection
/// is opened and passed to the callback handler.
///
/// It is important that `user_data` is the last field to not influence
/// offsets after monomorphization.
struct HandlerData<T> {
    message: MidiMessage,
    sysex_buffer: SysexBuffer,
    in_handle: Option<MidiInHandle>,
    ignore_flags: Ignore,
    callback: Box<dyn FnMut(u64, &[u32], &mut T) + Send + 'static>,
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
        let count = MidiInputPort::count();
        let mut result = Vec::with_capacity(count as usize);
        for i in 0..count {
            let port = match MidiInputPort::from_port_number(i) {
                Ok(p) => p,
                Err(_) => continue,
            };
            result.push(crate::common::MidiInputPort { imp: port });
        }
        result
    }

    pub fn port_count(&self) -> usize {
        MidiInputPort::count() as usize
    }

    pub fn port_name(&self, port: &MidiInputPort) -> Result<String, PortInfoError> {
        Ok(port.name.clone())
    }

    pub fn connect<F, T: Send>(
        self,
        port: &MidiInputPort,
        _port_name: &str,
        callback: F,
        data: T,
    ) -> Result<MidiInputConnection<T>, ConnectError<MidiInput>>
    where
        F: FnMut(u64, &[u32], &mut T) + Send + 'static,
    {
        let _ = (port, port_name, callback, data);
        Err(ConnectError::other("UMP MIDI 2.0 not implemented on this backend yet (see docs/ump-backend-compliance.md)", self))
    }
}

impl<T> MidiInputConnection<T> {
    pub fn close(mut self) -> (MidiInput, T) {
        self.close_internal();

        (
            MidiInput {
                ignore_flags: self.handler_data.ignore_flags,
            },
            self.handler_data.user_data.take().unwrap(),
        )
    }

    fn close_internal(&mut self) {
        // for information about this lock, see https://groups.google.com/forum/#!topic/mididev/6OUjHutMpEo
        let in_handle_lock = self.handler_data.in_handle.as_ref().unwrap().0.lock();

        // TODO: Call both reset and stop here? The difference seems to be that
        //       reset "returns all pending input buffers to the callback function"
        unsafe {
            midiInReset(*in_handle_lock);
            midiInStop(*in_handle_lock);
        }

        for i in 0..MIDIR_SYSEX_BUFFER_COUNT {
            let result;
            unsafe {
                result = midiInUnprepareHeader(
                    *in_handle_lock,
                    self.handler_data.sysex_buffer.0[i],
                    mem::size_of::<MIDIHDR>() as u32,
                );
                dealloc(
                    (*self.handler_data.sysex_buffer.0[i]).lpData.0 as *mut _,
                    Layout::from_size_align_unchecked(MIDIR_SYSEX_BUFFER_SIZE, 1),
                );
                // recreate the Box so that it will be dropped/deallocated at the end of this scope
                let _ = Box::from_raw(self.handler_data.sysex_buffer.0[i]);
            }

            if result != MMSYSERR_NOERROR {
                warn!("Warning: Ignoring error shutting down Windows MM input port (UnprepareHeader).");
            }
        }

        unsafe { midiInClose(*in_handle_lock) };
    }
}

impl<T> Drop for MidiInputConnection<T> {
    fn drop(&mut self) {
        // If user_data has been emptied, we know that we already have closed the connection
        if self.handler_data.user_data.is_some() {
            self.close_internal()
        }
    }
}

#[derive(Debug)]
pub struct MidiOutput;

#[derive(Clone)]
pub struct MidiOutputPort {
    name: String,
    interface_id: Box<[u16]>,
}

impl MidiOutputPort {
    pub fn id(&self) -> String {
        String::from_utf16_lossy(&self.interface_id[..self.interface_id.len() - 1])
    }
}

impl PartialEq for MidiOutputPort {
    fn eq(&self, other: &Self) -> bool {
        self.interface_id == other.interface_id
    }
}

pub struct MidiOutputConnection {
    out_handle: HMIDIOUT,
}

unsafe impl Send for MidiOutputConnection {}

impl MidiOutputPort {
    fn count() -> UINT {
        unsafe { midiOutGetNumDevs() }
    }

    /// Queries the interface ID. The result contains a terminating zero.
    fn interface_id(port_number: UINT) -> Result<Box<[u16]>, PortInfoError> {
        let mut buffer_size: ULONG = 0;
        let result = unsafe {
            midiOutMessage(
                Some(HMIDIOUT(port_number as *mut c_void)),
                DRV_QUERYDEVICEINTERFACESIZE,
                Some(&mut buffer_size as *mut _ as DWORD_PTR),
                None,
            )
        };
        if result == MMSYSERR_BADDEVICEID {
            return Err(PortInfoError::PortNumberOutOfRange);
        } else if result != MMSYSERR_NOERROR {
            return Err(PortInfoError::CannotRetrievePortName);
        }
        let mut buffer = Vec::<u16>::with_capacity(buffer_size as usize / 2);
        unsafe {
            let result = midiOutMessage(
                Some(HMIDIOUT(port_number as *mut c_void)),
                DRV_QUERYDEVICEINTERFACE,
                Some(buffer.as_mut_ptr() as DWORD_PTR),
                Some(buffer_size as DWORD_PTR),
            );
            if result == MMSYSERR_BADDEVICEID {
                return Err(PortInfoError::PortNumberOutOfRange);
            } else if result != MMSYSERR_NOERROR {
                return Err(PortInfoError::CannotRetrievePortName);
            }
            buffer.set_len(buffer_size as usize / 2);
        }
        //println!("{}", from_wide_ptr(buffer.as_ptr(), buffer.len()).to_string_lossy().into_owned());
        assert!(buffer.last().cloned() == Some(0));
        Ok(buffer.into_boxed_slice())
    }

    fn name(port_number: UINT) -> Result<String, PortInfoError> {
        let mut device_caps: MaybeUninit<MIDIOUTCAPSW> = MaybeUninit::uninit();
        let result = unsafe {
            midiOutGetDevCapsW(
                port_number as usize,
                device_caps.as_mut_ptr(),
                mem::size_of::<MIDIOUTCAPSW>() as u32,
            )
        };
        if result == MMSYSERR_BADDEVICEID {
            return Err(PortInfoError::PortNumberOutOfRange);
        } else if result != MMSYSERR_NOERROR {
            return Err(PortInfoError::CannotRetrievePortName);
        }
        let device_caps = unsafe { device_caps.assume_init() };
        let pname_ptr: *const [u16; 32] = std::ptr::addr_of!(device_caps.szPname);
        let output = from_wide_ptr(pname_ptr as *const _, 32)
            .to_string_lossy()
            .into_owned();
        Ok(output)
    }

    fn from_port_number(port_number: UINT) -> Result<Self, PortInfoError> {
        Ok(MidiOutputPort {
            name: Self::name(port_number)?,
            interface_id: Self::interface_id(port_number)?,
        })
    }

    fn current_port_number(&self) -> Option<UINT> {
        for i in 0..Self::count() {
            if let Ok(name) = Self::name(i) {
                if name != self.name {
                    continue;
                }
                if let Ok(id) = Self::interface_id(i) {
                    if id == self.interface_id {
                        return Some(i);
                    }
                }
            }
        }
        None
    }
}

impl MidiOutput {
    pub fn new(_client_name: &str) -> Result<Self, InitError> {
        Ok(MidiOutput)
    }

    pub(crate) fn ports_internal(&self) -> Vec<crate::common::MidiOutputPort> {
        let count = MidiOutputPort::count();
        let mut result = Vec::with_capacity(count as usize);
        for i in 0..count {
            let port = match MidiOutputPort::from_port_number(i) {
                Ok(p) => p,
                Err(_) => continue,
            };
            result.push(crate::common::MidiOutputPort { imp: port });
        }
        result
    }

    pub fn port_count(&self) -> usize {
        MidiOutputPort::count() as usize
    }

    pub fn port_name(&self, port: &MidiOutputPort) -> Result<String, PortInfoError> {
        Ok(port.name.clone())
    }

    pub fn connect(
        self,
        port: &MidiOutputPort,
        _port_name: &str,
    ) -> Result<MidiOutputConnection, ConnectError<MidiOutput>> {
        let _ = (port, port_name);
        Err(ConnectError::other("UMP MIDI 2.0 not implemented on this backend yet (see docs/ump-backend-compliance.md)", self))
    }
}

impl MidiOutputConnection {
    pub fn close(self) -> MidiOutput {
        // The actual closing is done by the implementation of Drop
        MidiOutput // In this API this is a noop
    }

    pub fn send(&mut self, words: &[u32]) -> Result<(), SendError> {
        let _ = words;
        Err(SendError::Other(
            "UMP MIDI 2.0 not implemented on this backend yet (see docs/ump-backend-compliance.md)",
        ))
    }
}

impl Drop for MidiOutputConnection {
    fn drop(&mut self) {
        unsafe {
            midiOutReset(self.out_handle);
            midiOutClose(self.out_handle);
        }
    }
}
