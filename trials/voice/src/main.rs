//! Trial 1 of the interface redesign: which engine turns speech into words
//! on the device, as the words are spoken.
//!
//! `voice-trial hear` feeds an engine a set of recorded thoughts at the
//! speed a microphone would, and writes one line of JSON for each: every
//! change to what would be on screen and when, the engine's working time,
//! and when it thought the speaker had finished. `scripts/analyse.py` turns
//! those lines into the trial's figures.

mod audio;
mod live;
mod mem;
#[cfg(feature = "opus")]
mod ogg_opus;
#[cfg(feature = "sherpa")]
mod sherpa_engine;
#[cfg(feature = "whisper")]
mod whisper_engine;
#[cfg(feature = "whisper")]
mod words;

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde_json::json;

use live::Live;

fn main() {
    let mut args = std::env::args().skip(1);
    let command = args.next().unwrap_or_default();
    let options = Flags::parse(args);
    let result = match command.as_str() {
        "hear" => hear(&options),
        #[cfg(feature = "whisper")]
        "whisper-vad" => whisper_vad(&options),
        #[cfg(feature = "opus")]
        "opus" => ogg_opus::encode_file(
            Path::new(&options.need("in")),
            Path::new(&options.need("out")),
            options.number("bitrate", 24_000),
        ),
        _ => Err(USAGE.to_owned()),
    };
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

const USAGE: &str = "\
voice-trial hear --engine whisper|sherpa --model PATH --set MANIFEST --out FILE
    [--threads 4] [--chunk-ms 100] [--tail 3.0] [--pretend-clock] [--only ID,ID]
    [--high-priority] [--lead 0.0]
    whisper: [--step-ms 500] [--short-window] [--trim 20]
    sherpa:  [--fp32] [--decoding greedy_search] [--end-after 1.6]
             [--vad silero_vad.onnx] [--second-pass MOONSHINE_FOLDER]
voice-trial whisper-vad --model ggml-silero.bin --set MANIFEST --out FILE
voice-trial opus --in FILE.wav --out FILE.opus [--bitrate 24000]

MANIFEST is a tab-separated file: an id, then a WAV file beside it (16 kHz mono).";

/// `--name value` pairs and bare `--name` switches.
struct Flags(HashMap<String, String>);

impl Flags {
    fn parse(args: impl Iterator<Item = String>) -> Self {
        let mut flags = HashMap::new();
        let mut args = args.peekable();
        while let Some(arg) = args.next() {
            let Some(name) = arg.strip_prefix("--") else {
                continue;
            };
            let value = match args.peek() {
                Some(next) if !next.starts_with("--") => args.next().unwrap_or_default(),
                _ => "true".to_owned(),
            };
            flags.insert(name.to_owned(), value);
        }
        Self(flags)
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.0.get(name).map(String::as_str)
    }

    fn need(&self, name: &str) -> String {
        self.get(name)
            .unwrap_or_else(|| {
                eprintln!("--{name} is needed\n{USAGE}");
                std::process::exit(2);
            })
            .to_owned()
    }

    fn has(&self, name: &str) -> bool {
        self.0.contains_key(name)
    }

    fn number<T: std::str::FromStr>(&self, name: &str, default: T) -> T {
        self.get(name)
            .and_then(|v| v.parse().ok())
            .unwrap_or(default)
    }
}

/// A fixed piece of work on one thread, timed: how fast the machine is for
/// this process at this moment. Other work on the machine and a throttled
/// processor both show up here, so runs can be compared.
fn calibrate() -> f64 {
    let began = Instant::now();
    let mut state: u32 = 1;
    let mut sum = 0.0_f64;
    for _ in 0..40_000_000_u32 {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        sum += f64::from(state >> 8).sqrt();
    }
    std::hint::black_box(sum);
    began.elapsed().as_secs_f64()
}

/// The thoughts of a set: each one's id and sound.
fn read_set(manifest: &Path, only: Option<&str>) -> Result<Vec<(String, Vec<f32>)>, String> {
    let text =
        std::fs::read_to_string(manifest).map_err(|e| format!("{}: {e}", manifest.display()))?;
    let folder = manifest.parent().unwrap_or_else(|| Path::new("."));
    let only: Option<Vec<&str>> = only.map(|ids| ids.split(',').collect());
    let mut set = Vec::new();
    for line in text
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
    {
        let mut fields = line.split('\t');
        let (Some(id), Some(file)) = (fields.next(), fields.next()) else {
            return Err(format!(
                "{}: a line needs an id and a file",
                manifest.display()
            ));
        };
        if only.as_ref().is_some_and(|ids| !ids.contains(&id)) {
            continue;
        }
        set.push((id.to_owned(), audio::read_wav(&folder.join(file))?));
    }
    Ok(set)
}

fn load_engine(flags: &Flags) -> Result<Box<dyn Live>, String> {
    let model = PathBuf::from(flags.need("model"));
    let threads: i32 = flags.number("threads", 4);
    match flags.need("engine").as_str() {
        #[cfg(feature = "whisper")]
        "whisper" => {
            let options = whisper_engine::Options {
                threads,
                step_ms: flags.number("step-ms", 500),
                short_window: flags.has("short-window"),
                trim_seconds: flags.number("trim", 20.0),
            };
            Ok(Box::new(whisper_engine::SlidingWhisper::load(
                &model, options,
            )?))
        }
        #[cfg(feature = "sherpa")]
        "sherpa" => {
            let options = sherpa_engine::Options {
                threads,
                int8: !flags.has("fp32"),
                decoding: flags.get("decoding").unwrap_or("greedy_search").to_owned(),
                end_after_seconds: flags.number("end-after", 1.6),
                vad: flags.get("vad").map(PathBuf::from),
                second_pass: flags.get("second-pass").map(PathBuf::from),
            };
            Ok(Box::new(sherpa_engine::Streaming::load(&model, &options)?))
        }
        other => Err(format!("this build has no engine called {other:?}")),
    }
}

fn hear(flags: &Flags) -> Result<(), String> {
    let set = read_set(Path::new(&flags.need("set")), flags.get("only"))?;
    let out_path = flags.need("out");
    let mut out = std::fs::File::create(&out_path).map_err(|e| format!("{out_path}: {e}"))?;
    let chunk_ms: usize = flags.number("chunk-ms", 100);
    let tail = audio::quiet(flags.number("tail", 3.0));
    let lead = audio::quiet(flags.number("lead", 0.0));
    let real_time = !flags.has("pretend-clock");
    if flags.has("high-priority") {
        mem::run_ahead_of_other_work();
    }

    let before = Instant::now();
    let mut engine = load_engine(flags)?;
    let load_seconds = before.elapsed().as_secs_f64();
    let line = json!({
        "kind": "run",
        "flags": flags.0,
        "target": format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS),
        "load_seconds": load_seconds,
        "peak_bytes_after_load": mem::peak_bytes(),
        "real_time": real_time,
        "calibration_seconds": calibrate(),
    });
    writeln!(out, "{line}").map_err(|e| e.to_string())?;

    let mut speech_seconds = 0.0;
    let mut compute_seconds = 0.0;
    for (id, speech) in &set {
        engine.reset();
        let mut heard = live::hear(
            engine.as_mut(),
            id,
            &lead,
            speech,
            &tail,
            chunk_ms,
            real_time,
        );
        let began = Instant::now();
        if let Some(text) = engine.second_pass(speech) {
            heard.second_pass = Some(live::SecondPass {
                text,
                seconds: began.elapsed().as_secs_f64(),
            });
        }
        speech_seconds += heard.fed_seconds;
        compute_seconds += heard.compute_seconds;
        eprintln!(
            "{id}: {:.1} s fed, {:.2} s of work, worst lag {:.2} s: {} | {}",
            heard.fed_seconds,
            heard.compute_seconds,
            heard.worst_lag_seconds,
            heard.last.settled,
            heard.last.tentative
        );
        let mut line = serde_json::to_value(&heard).map_err(|e| e.to_string())?;
        line["kind"] = json!("thought");
        writeln!(out, "{line}").map_err(|e| e.to_string())?;
    }
    let line = json!({
        "kind": "end",
        "fed_seconds": speech_seconds,
        "compute_seconds": compute_seconds,
        "peak_bytes": mem::peak_bytes(),
        "calibration_seconds": calibrate(),
    });
    writeln!(out, "{line}").map_err(|e| e.to_string())?;
    eprintln!(
        "{:.0} s fed, {:.1} s of work ({:.3} of real time), peak {} MiB",
        speech_seconds,
        compute_seconds,
        compute_seconds / speech_seconds.max(1e-9),
        mem::peak_bytes().unwrap_or(0) / (1 << 20)
    );
    Ok(())
}

/// Silero's voice-activity model as whisper.cpp runs it, over each thought
/// with the same quiet after it: one probability of speech per 32 ms.
#[cfg(feature = "whisper")]
fn whisper_vad(flags: &Flags) -> Result<(), String> {
    let set = read_set(Path::new(&flags.need("set")), flags.get("only"))?;
    let out_path = flags.need("out");
    let mut out = std::fs::File::create(&out_path).map_err(|e| format!("{out_path}: {e}"))?;
    let model = PathBuf::from(flags.need("model"));
    let tail = audio::quiet(flags.number("tail", 3.0));
    for (id, speech) in &set {
        let all: Vec<f32> = speech.iter().chain(&tail).copied().collect();
        let began = Instant::now();
        let probabilities = whisper_engine::vad_probabilities(&model, 1, &all)?;
        let line = json!({
            "kind": "vad",
            "id": id,
            "speech_seconds": speech.len() as f64 / audio::RATE as f64,
            "seconds": began.elapsed().as_secs_f64(),
            "frame_seconds": 512.0 / audio::RATE as f64,
            "probabilities": probabilities,
        });
        writeln!(out, "{line}").map_err(|e| e.to_string())?;
    }
    Ok(())
}
