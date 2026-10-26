//! Lists the encoders that work on this machine, per codec.

fn main() {
    let mut found: Vec<_> = videode_export::detect_all().into_iter().collect();
    found.sort();
    for (codec, encoders) in found {
        let names: Vec<String> = encoders
            .iter()
            .map(|e| {
                let kind = if videode_export::is_hardware(e) {
                    "hardware"
                } else {
                    "software"
                };
                format!("{} ({e}, {kind})", videode_export::encoder_label(e))
            })
            .collect();
        println!(
            "{codec:12} {}",
            if names.is_empty() {
                "none".into()
            } else {
                names.join(", ")
            }
        );
    }
}
