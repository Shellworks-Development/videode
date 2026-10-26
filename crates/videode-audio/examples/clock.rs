//! Plays silence for a second and prints the playback clock: a check that
//! the default output device works. `cargo run -p videode-audio --example clock`

fn main() -> Result<(), String> {
    let mut engine = videode_audio::AudioEngine::start()?;
    println!("device rate: {} Hz", engine.rate());
    engine.play(10., videode_audio::Mix::default());
    for _ in 0..5 {
        std::thread::sleep(std::time::Duration::from_millis(250));
        println!("position: {:.3} s", engine.position().unwrap_or(f64::NAN));
    }
    engine.stop();
    Ok(())
}
