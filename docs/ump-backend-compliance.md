# UMP backend compliance

Gate each midir backend as **UMP-ready** only when every required row is green. Until then the backend must **stub** `connect` / `send` with an explicit error (no silent MIDI 1.0 down-conversion).

## Cross-cutting — MIDI Association UMP + MIDI 2.0 Protocol (M2-104, v1.1.x)

Spec: [Universal MIDI Packet (UMP) and MIDI 2.0 Protocol](https://midi.org/universal-midi-packet-ump-and-midi-2-0-protocol-specification)

| Check | Status |
|-------|--------|
| Public API uses 32-bit UMP words (not MIDI 1.0 byte stream) | **pass** (crate-wide) |
| Message length follows UMP Message Type (1–4 words) — transport does not re-frame | **pass** (raw words) |
| SysEx8 / Data128 (MT `0x5`) round-trip without SysEx7 conversion | **pass** on Apple; pending elsewhere |
| No private framing layer inside midir | **pass** |

## Apple (macOS + iOS) — CoreMIDI MIDI 2.0

| Check | Status | Notes |
|-------|--------|-------|
| `MIDIInputPortCreateWithProtocol` / `kMIDIProtocol_2_0` | **pass** | `Protocol::Midi20` |
| Send/receive via `MIDIEventList` + `MIDISendEventList` | **pass** | `EventBuffer` / receive block |
| Native-endian UMP words | **pass** | CoreMIDI word layout |
| Stable unique id for ports | **pass** | `unique_id()` → port `id()` |

## Linux — ALSA UMP / kernel MIDI 2.0

Spec: [MIDI 2.0 on Linux](https://docs.kernel.org/sound/designs/midi-2.0.html)

| Check | Status | Notes |
|-------|--------|-------|
| Prefer `/dev/snd/ump*` / UMP sequencer (not legacy rawmidi alone) | **stub** | connect/send error |
| CPU-native-endian 32-bit UMP read/write | **stub** | |
| Fail closed if kernel lacks UMP (no silent MIDI 1.0) | **pass** (stub) | |

## Windows — Windows MIDI Services / USB MIDI 2.0

| Check | Status | Notes |
|-------|--------|-------|
| UMP via MIDI Services App SDK / WinRT MIDI 2 (not WinMM bytes as Gestures path) | **stub** | |
| No silent MIDI 1.0 downscale for ports claimed as UMP | **pass** (stub) | |

## Exit criteria

- Checklist cites the API/spec used.
- Smoke: send/receive at least one 4-word UMP (Data128-shaped) on that OS when CI/hardware allows.
- Otherwise documented stub (this file) + issue link when filed.
