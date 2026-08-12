use std::error::Error;
use std::io::{stdin, stdout, Write};
use std::thread::sleep;
use std::time::Duration;

use midir::{MidiOutput, MidiOutputPort};

fn main() {
    match run() {
        Ok(_) => (),
        Err(err) => println!("Error: {}", err),
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let midi_out = MidiOutput::new("midir ump test")?;
    let out_ports = midi_out.ports();
    let out_port: &MidiOutputPort = match out_ports.len() {
        0 => return Err("no output port found".into()),
        1 => {
            println!("Selecting the only available output port: {}", midi_out.port_name(&out_ports[0]).unwrap());
            &out_ports[0]
        }
        _ => {
            println!("Available output ports:");
            for (i, p) in out_ports.iter().enumerate() {
                println!("{}: {}", i, midi_out.port_name(p).unwrap());
            }
            print!("Please select output port: ");
            stdout().flush()?;
            let mut input = String::new();
            stdin().read_line(&mut input)?;
            out_ports
                .get(input.trim().parse::<usize>()?)
                .ok_or("invalid output port selected")?
        }
    };

    println!("Opening connection");
    let mut conn_out = midi_out.connect(out_port, "midir-ump-test")?;
    println!("Connection open. Sending one Data128-shaped UMP (4 words), then a MIDI 1.0 Note On UMP…");
    sleep(Duration::from_millis(200));

    // One complete SysEx8-style Data128 UMP (MT=0x5) — illustrative words only.
    conn_out.send(&[0x5001_0000, 0x0102_0304, 0x0506_0708, 0x090A_0B0C])?;
    sleep(Duration::from_millis(200));
    // MIDI 1.0 Note On middle C in UMP (MT=0x2), velocity 100.
    conn_out.send(&[0x2090_3C64])?;
    sleep(Duration::from_millis(200));
    conn_out.send(&[0x2080_3C00])?;

    println!("Done");
    Ok(())
}
