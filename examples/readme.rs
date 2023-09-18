use voicevox_dyn::{AccelerationMode, VoiceVox};

const SPEAKER_ID: u32 = 4;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let threads = std::thread::available_parallelism()?.get() as u16;

    let mut vv = VoiceVox::load()?;
    vv.init(AccelerationMode::Auto, threads, false)?;
    vv.load_model(SPEAKER_ID)?;

    let wav = vv.tts("こんにちは", SPEAKER_ID, Default::default())?;

    std::fs::write("audio.wav", wav.as_slice())?;

    Ok(())
}
