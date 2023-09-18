# voicevox without hassle
Downloads VOICEVOX CORE and dynamically loads it at runtime.

This crate makes it as easy as possible to use voicevox in rust, in particular, it is possible to distribute a single binary that sets up
voicevox itself and is also able to run it.

## Example
```rust
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

```

### Alternatives

If you prefer to dynamically link voicevox instead, I recommend using [vvcore](https://github.com/iwase22334/voicevox-core-rs).