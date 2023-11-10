//! # VoiceVox without hassle
//! Downloads VOICEVOX CORE and dynamically loads it at runtime.
//!
//! The intent of this crate is to make using voicevox as easy as possible
//! and in particular making it easy to distribute a single binary that
//! sets up voicevox itself and is also able to run it.
//!
//! ### Alternatives
//! If you prefer to dynamically link voicevox instead, I recommend using [vvcore](https://github.com/iwase22334/voicevox-core-rs).

use color_eyre::eyre::bail;
use libloading::Symbol;
use std::{ffi::OsStr, path::PathBuf, process::Stdio};
use tracing::info;

pub struct VoiceVox {
    fns: VoiceVoxFns,
    init: bool,
}

#[ouroboros::self_referencing]
pub struct VoiceVoxFns {
    lib: libloading::Library,
    #[covariant]
    #[borrows(lib)]
    init: Symbol<'this, unsafe extern "C" fn(InitOptions) -> ResultCode>,
    #[covariant]
    #[borrows(lib)]
    load_model: Symbol<'this, unsafe extern "C" fn(u32) -> ResultCode>,
    #[covariant]
    #[borrows(lib)]
    tts: Symbol<'this, TtsFn>,
    #[covariant]
    #[borrows(lib)]
    wav_free: Symbol<'this, unsafe extern "C" fn(*mut u8)>,
}

type TtsFn = unsafe extern "C" fn(
    text: *const ::std::os::raw::c_char,
    speaker_id: u32,
    options: TtsOptions,
    output_wav_length: *mut usize,
    output_wav: *mut *mut u8,
) -> ResultCode;

impl VoiceVox {
    /// Creates a new VoiceVox instance and downloads all required files for running
    /// voicevox into the directory of the executable.
    ///
    /// Note that `VoiceVox` is not initialized automatically, as initialization is expensive. To initialize `VoiceVox` call [`VoiceVox::init`].
    ///
    /// After initialization, `VoiceVox` can be used to synthesize speech with [`VoiceVox::tts`].
    ///
    /// By default the CPU runtime for voicevox is downloaded. For cuda support,
    /// use [`VoiceVox::new_with_args`] with `["--device", "cuda"]` as the argument.
    pub fn load() -> color_eyre::Result<Self> {
        Self::load_with_args(std::iter::empty::<&str>())
    }

    /// Same as [`VoiceVox::new`] but allows passing arguments to the voicevox downloader.
    ///
    /// See [here](https://github.com/VOICEVOX/voicevox_core/blob/6a662757b8d42fc5d0902364b1d549684b50b5bc/crates/download/src/main.rs#L50) for a list of possible arguments.
    pub fn load_with_args<S: AsRef<OsStr>>(
        args: impl IntoIterator<Item = S>,
    ) -> color_eyre::Result<Self> {
        let exe_path = download_path()?;
        #[cfg(target_os = "windows")]
        let dll = exe_path.join("voicevox_core.dll");
        #[cfg(target_os = "macos")]
        let dll = exe_path.join("libvoicevox_core.dylib");
        #[cfg(target_os = "linux")]
        let dll = exe_path.join("libvoicevox_core.so");

        if !dll.exists() {
            // get the downloader
            info!("Downloading voicevox downloader.");
            let mut reader = ureq::get(&voicevox_downloader_url()?).call()?.into_reader();
            let downloader_path = exe_path.join("voicevox_downloader");
            let file = std::fs::File::create(&downloader_path)?;
            std::io::copy(&mut reader, &mut std::io::BufWriter::new(file))?;

            #[cfg(target_family = "unix")]
            std::process::Command::new("chmod")
                .arg("+x")
                .arg(&downloader_path)
                .output()
                .unwrap();

            // use the downloader
            let mut child = std::process::Command::new(downloader_path)
                .args([
                    "-o",
                    exe_path.to_str().ok_or(color_eyre::eyre::eyre!(
                        "failed to convert {:?} to str",
                        exe_path
                    ))?,
                ])
                .args(args)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?;

            info!("Downloading voicevox. This may take a while, roughly 700MB of data will be downloaded.");
            // This doesn't output the progress bars, so not very useful.
            // let mut out = child.stdout.take().unwrap();
            // let mut err = child.stderr.take().unwrap();
            // std::thread::spawn(move || {
            //     std::io::copy(&mut out, &mut std::io::stderr()).unwrap();
            // });
            // std::thread::spawn(move || {
            //     std::io::copy(&mut err, &mut std::io::stdout()).unwrap();
            // });

            child.wait()?;
        }

        unsafe {
            let lib = libloading::Library::new(dll).unwrap();

            Ok(Self {
                fns: VoiceVoxFns::new(
                    lib,
                    |lib| lib.get(b"voicevox_initialize").unwrap(),
                    |lib| lib.get(b"voicevox_load_model").unwrap(),
                    |lib| lib.get(b"voicevox_tts").unwrap(),
                    |lib| lib.get(b"voicevox_wav_free").unwrap(),
                ),
                init: false,
            })
        }
    }

    /// Initializes the voicevox runtime. This is expensive when called with
    /// `load_all_models = true`, so it is recommended to instead load only
    /// the models you need with [`VoiceVox::load_model`].
    pub fn init(
        &mut self,
        acceleration_mode: AccelerationMode,
        cpu_num_threads: u16,
        load_all_models: bool,
    ) -> color_eyre::Result<()> {
        let opts = InitOptions::new(acceleration_mode, cpu_num_threads, load_all_models)?;

        info!("Initializing voicevox. This can take a while.");
        if self.init {
            return Ok(());
        }
        match unsafe { (self.fns.borrow_init())(opts) } {
            ResultCode::Ok => {
                self.init = true;
                Ok(())
            }
            e => Err(e.into()),
        }
    }

    /// Loads one of the models.
    pub fn load_model(&self, speaker_id: u32) -> Result<(), ResultCode> {
        match unsafe { (self.fns.borrow_load_model())(speaker_id) } {
            ResultCode::Ok => Ok(()),
            e => Err(e),
        }
    }

    /// Synthesizes speech from the given text.
    ///
    /// To get a list of speaker ids, run the [`VoiceVox::new`] once
    /// and check `model/metas.json` in the directory of the executable.
    pub fn tts(
        &self,
        text: impl AsRef<str>,
        speaker_id: u32,
        opts: TtsOptions,
    ) -> Result<CPointerWrap<u8>, ResultCode> {
        let text = text.as_ref();
        info!("Synthesizing speech from: {}", text);

        let text = std::ffi::CString::new(text).unwrap();
        let mut output_wav_length = 0;
        let mut output_wav = std::ptr::null_mut();

        match unsafe {
            (self.fns.borrow_tts())(
                text.as_ptr(),
                speaker_id,
                opts,
                &mut output_wav_length,
                &mut output_wav,
            )
        } {
            ResultCode::Ok => Ok(CPointerWrap::new(
                output_wav,
                output_wav_length,
                self.fns.borrow_wav_free(),
            )),
            e => Err(e),
        }
    }
}

fn download_path() -> color_eyre::Result<PathBuf> {
    let exe_path = std::env::current_exe()?;
    Ok(exe_path
        .parent()
        .ok_or(color_eyre::eyre::eyre!("exe path has no parent directory"))?
        .to_owned())
}

fn voicevox_downloader_url() -> color_eyre::Result<String> {
    let os = match std::env::consts::OS {
        os @ "windows" | os @ "linux" => os,
        "macos" => "osx",
        _ => bail!("unsupported os"),
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        _ => bail!("unsupported arch"),
    };
    let extension = match os {
        "windows" => ".exe",
        _ => "",
    };
    let base = "https://github.com/VOICEVOX/voicevox_core/releases/latest/download/download-";
    Ok(format!("{base}{os}-{arch}{extension}"))
}

#[repr(C)]
#[derive(Default, Debug, Copy, Clone)]
pub struct TtsOptions {
    pub kana: bool,
    pub enable_interrogative_upspeak: bool,
}

#[repr(C)]
#[derive(Debug, Clone)]
pub struct InitOptions {
    acceleration_mode: i32,
    cpu_num_threads: u16,
    load_all_models: bool,
    open_jtalk_dict_dir: *mut ::std::os::raw::c_char,
}

#[derive(Debug, Clone, Copy)]
pub enum AccelerationMode {
    Auto,
    Cpu,
    Gpu,
}

impl InitOptions {
    pub fn new(
        acceleration_mode: AccelerationMode,
        cpu_num_threads: u16,
        load_all_models: bool,
    ) -> color_eyre::Result<Self> {
        let p = download_path()?
            .join("open_jtalk_dic_utf_8-1.11")
            .canonicalize()?;
        let open_jtalk_dict_dir = p
            .to_str()
            .ok_or(color_eyre::eyre::eyre!("failed to convert {:?} to str", p))?;

        Ok(Self {
            acceleration_mode: match acceleration_mode {
                AccelerationMode::Auto => 0,
                AccelerationMode::Cpu => 1,
                AccelerationMode::Gpu => 2,
            },
            cpu_num_threads,
            load_all_models,
            open_jtalk_dict_dir: std::ffi::CString::new(open_jtalk_dict_dir)
                .unwrap()
                .into_raw(),
        })
    }
}

impl Drop for InitOptions {
    fn drop(&mut self) {
        drop(unsafe { std::ffi::CString::from_raw(self.open_jtalk_dict_dir) })
    }
}

#[repr(i32)]
#[derive(Debug, PartialEq, Eq)]
pub enum ResultCode {
    /// Success
    Ok = 0,
    /// Failed to load Open JTalk dictionary file
    NotLoadedOpenjtalkDictError = 1,
    /// Failed to load the model
    LoadModelError = 2,
    /// Failed to get supported device information
    GetSupportedDevicesError = 3,
    /// GPU mode is not supported
    GpuSupportError = 4,
    /// Failed to load meta information
    LoadMetasError = 5,
    /// Status is uninitialized
    UninitializedStatusError = 6,
    /// Invalid speaker ID specified
    InvalidSpeakerIdError = 7,
    /// Invalid model index specified
    InvalidModelIndexError = 8,
    /// Inference failed
    InferenceError = 9,
    /// Failed to output context labels
    ExtractFullContextLabelError = 10,
    /// Invalid UTF-8 string input
    InvalidUtf8InputError = 11,
    /// Failed to parse Aquestalk-style text
    ParseKanaError = 12,
    /// Invalid AudioQuery
    InvalidAudioQueryError = 13,
}

impl std::fmt::Display for ResultCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            ResultCode::Ok => "Success",
            ResultCode::NotLoadedOpenjtalkDictError => "Failed to load Open JTalk dictionary file",
            ResultCode::LoadModelError => "Failed to load the model",
            ResultCode::GetSupportedDevicesError => "Failed to get supported device information",
            ResultCode::GpuSupportError => "GPU mode is not supported",
            ResultCode::LoadMetasError => "Failed to load meta information",
            ResultCode::UninitializedStatusError => "Status is uninitialized",
            ResultCode::InvalidSpeakerIdError => "Invalid speaker ID specified",
            ResultCode::InvalidModelIndexError => "Invalid model index specified",
            ResultCode::InferenceError => "Inference failed",
            ResultCode::ExtractFullContextLabelError => "Failed to output context labels",
            ResultCode::InvalidUtf8InputError => "Invalid UTF-8 string input",
            ResultCode::ParseKanaError => "Failed to parse Aquestalk-style text",
            ResultCode::InvalidAudioQueryError => "Invalid AudioQuery",
        };
        write!(f, "{}", s)
    }
}

impl std::error::Error for ResultCode {}

/// Once dropped the memory is freed.
pub struct CPointerWrap<'a, T> {
    bytes: *mut T,
    length: usize,
    free_fn: &'a Symbol<'a, unsafe extern "C" fn(*mut T)>,
}

impl<'a, T> CPointerWrap<'a, T> {
    pub fn new(
        bytes: *mut T,
        length: usize,
        free_fn: &'a Symbol<'a, unsafe extern "C" fn(*mut T)>,
    ) -> Self {
        Self {
            bytes,
            length,
            free_fn,
        }
    }

    pub fn as_slice(&self) -> &[T] {
        unsafe { std::slice::from_raw_parts(self.bytes, self.length) }
    }
}

impl<'a, T> Drop for CPointerWrap<'a, T> {
    fn drop(&mut self) {
        unsafe { (self.free_fn)(self.bytes) };
    }
}
