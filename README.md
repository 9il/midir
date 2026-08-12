# midir (UMP fork)

**This is a public fork of [Boddlnagg/midir](https://github.com/Boddlnagg/midir) maintained at [github.com/9il/midir](https://github.com/9il/midir).**

Breaking change vs upstream: the public API carries **Universal MIDI Packets (UMP)** as native-endian `u32` words — **not** classic MIDI 1.0 byte streams. It is **not** wire-compatible with upstream midir 0.10/0.11.

There is **no** dependency on the [`midi2`](https://crates.io/crates/midi2) crate; this crate only transports raw UMP words.

## API (UMP)

- Input callback: `FnMut(u64 /*µs*/, &[u32] /*UMP words*/, &mut T)`
- Output: `MidiOutputConnection::send(words: &[u32])`
- Apple (macOS / iOS): CoreMIDI `Protocol::Midi20` + `MIDIEventList` / `MIDISendEventList`
- Linux / Windows / Jack / WebMIDI / Android: enumerate may work; **connect/send are stubs** until each backend passes [`docs/ump-backend-compliance.md`](docs/ump-backend-compliance.md)

## Platform status

| Backend | Enumerate | UMP I/O |
|---------|-----------|---------|
| CoreMIDI (macOS, iOS) | yes | yes (MIDI 2.0 EventList) |
| ALSA | yes* | stub (pending ALSA UMP) |
| WinMM / WinRT | yes* | stub (pending Windows MIDI Services UMP) |
| Jack / WebMIDI / Android | yes* | stub |

\*Legacy enumeration code may still list ports; opening for UMP returns a clear error until compliance is green.

## License

MIT (same as upstream midir).
