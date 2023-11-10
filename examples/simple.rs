use tracing::info;
use voicevox_dyn::{AccelerationMode, VoiceVox};

const SPEAKER_ID: u32 = 4;

fn main() -> color_eyre::Result<()> {
    tracing_subscriber::fmt::init();
    color_eyre::install()?;
    let threads = std::thread::available_parallelism()?.get() as u16;

    let mut vv = VoiceVox::load()?;
    vv.init(AccelerationMode::Auto, threads, false)?;
    vv.load_model(SPEAKER_ID)?;

    let now = std::time::Instant::now();
    let wav = vv.tts("ステキだね", SPEAKER_ID, Default::default())?;
    info!("tts took {:?}", now.elapsed());

    std::fs::write("audio.wav", wav.as_slice())?;

    Ok(())
}
