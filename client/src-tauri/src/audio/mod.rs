//! 録音とリサンプリング (design.md §5.1, §5.4)。
//!
//! cpal で既定 (または設定で選んだ) マイクを開き、rubato で 16kHz モノラルへ変換して
//! PCM16LE の 100ms チャンクに切り出す。「マイクを常時開いておく」設定時は
//! 直近300msを [`PreRollBuffer`] に保持し、押下時にそこから送り始める。
//!
//! 実機のマイクを開く部分 ([`open_input_stream`]) は cpal のデバイスが必要なため
//! 単体テストの対象外 (README に明記)。それ以外の純粋なロジック
//! (ダウンミックス・リサンプリング・PCM 化・チャンク分割・プリロール) はテストする。

use audioadapter_buffers::direct::SequentialSliceOfVecs;
use rubato::{FixedSync, Resampler as _};

pub const TARGET_SAMPLE_RATE: u32 = 16_000;
pub const CHUNK_MS: u32 = 100;
pub const PRE_ROLL_MS: u32 = 300;
/// PCM16LE モノラル 16kHz の 100ms チャンクのバイト数 (design.md §3.1「約3,200バイト」)。
pub const CHUNK_BYTES: usize = (TARGET_SAMPLE_RATE as usize / 1000) * (CHUNK_MS as usize) * 2;

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("リサンプラーの初期化/実行に失敗: {0}")]
    Resampler(String),
    #[error("入力デバイスの取得/設定に失敗: {0}")]
    Device(String),
    #[error("録音ストリームの構築に失敗: {0}")]
    Stream(String),
}

/// 複数チャンネルの1フレームをモノラルへダウンミックス (単純平均)。
pub fn downmix_to_mono(frame: &[f32]) -> f32 {
    if frame.is_empty() {
        return 0.0;
    }
    frame.iter().sum::<f32>() / frame.len() as f32
}

/// インターリーブされた多チャンネル PCM (f32, -1.0..=1.0) をモノラルの列に変換する。
pub fn interleaved_to_mono(samples: &[f32], channels: usize) -> Vec<f32> {
    if channels == 0 {
        return Vec::new();
    }
    if channels == 1 {
        return samples.to_vec();
    }
    samples
        .chunks(channels)
        .map(downmix_to_mono)
        .collect()
}

/// f32 (-1.0..=1.0) を PCM16LE バイト列へ変換する。範囲外の値はクリップする。
pub fn encode_pcm16le(samples: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(samples.len() * 2);
    for &s in samples {
        let clamped = s.clamp(-1.0, 1.0);
        let value = (clamped * i16::MAX as f32).round() as i16;
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}

/// 任意サイズで届く PCM バイト列を、固定サイズ ([`CHUNK_BYTES`]) のチャンクに切り出す。
/// 端数は次回に持ち越す (100ms 単位でサーバーへ送るため)。
#[derive(Debug, Default)]
pub struct ChunkAccumulator {
    pending: Vec<u8>,
}

impl ChunkAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    /// バイト列を追加し、切り出せる完全なチャンクをすべて返す。
    pub fn push(&mut self, bytes: &[u8]) -> Vec<Vec<u8>> {
        self.pending.extend_from_slice(bytes);
        let mut chunks = Vec::new();
        while self.pending.len() >= CHUNK_BYTES {
            let chunk = self.pending.drain(..CHUNK_BYTES).collect();
            chunks.push(chunk);
        }
        chunks
    }

    /// 発話終了時に残った端数を1チャンクとして取り出す (空なら None)。
    pub fn flush(&mut self) -> Option<Vec<u8>> {
        if self.pending.is_empty() {
            None
        } else {
            Some(std::mem::take(&mut self.pending))
        }
    }
}

/// 「マイクを常時開いておく」設定用の直近 [`PRE_ROLL_MS`] リングバッファ (design.md §5.4)。
#[derive(Debug)]
pub struct PreRollBuffer {
    max_bytes: usize,
    data: std::collections::VecDeque<u8>,
}

impl PreRollBuffer {
    pub fn new() -> Self {
        let max_bytes = (TARGET_SAMPLE_RATE as usize / 1000) * (PRE_ROLL_MS as usize) * 2;
        Self {
            max_bytes,
            data: std::collections::VecDeque::with_capacity(max_bytes),
        }
    }

    pub fn push(&mut self, bytes: &[u8]) {
        self.data.extend(bytes.iter().copied());
        while self.data.len() > self.max_bytes {
            self.data.pop_front();
        }
    }

    /// 押下時点で保持している直近分をそのまま取り出す (バッファはクリアしない。継続録音のため)。
    pub fn snapshot(&self) -> Vec<u8> {
        self.data.iter().copied().collect()
    }

    pub fn max_bytes(&self) -> usize {
        self.max_bytes
    }
}

impl Default for PreRollBuffer {
    fn default() -> Self {
        Self::new()
    }
}

/// 入力サンプルレート → 16kHz モノラルへの同期リサンプラー。
pub struct MonoResampler {
    inner: rubato::Fft<f32>,
    input_frames: usize,
    output_frames_max: usize,
    /// 呼び出しごとに確保し直さないための再利用バッファ (レビュー指摘7: cpal のコールバックは
    /// リアルタイム性が要るスレッドなので、毎回のアロケーションを避ける)。1チャンネル分。
    input_scratch: Vec<Vec<f32>>,
    output_scratch: Vec<Vec<f32>>,
}

impl MonoResampler {
    pub fn new(input_rate: u32, input_frames: usize) -> Result<Self, AudioError> {
        let inner = rubato::Fft::<f32>::new(
            input_rate as usize,
            TARGET_SAMPLE_RATE as usize,
            input_frames,
            1,
            FixedSync::Input,
        )
        .map_err(|e| AudioError::Resampler(e.to_string()))?;
        let output_frames_max = inner.output_frames_max();
        Ok(Self {
            inner,
            input_frames,
            output_frames_max,
            input_scratch: vec![Vec::with_capacity(input_frames)],
            output_scratch: vec![vec![0f32; output_frames_max]],
        })
    }

    /// 呼び出し側が期待される入力フレーム数 (コンストラクタで指定した `input_frames`)。
    pub fn input_frames(&self) -> usize {
        self.input_frames
    }

    /// `mono_samples.len()` は `input_frames()` と一致していなければならない
    /// (末尾の余りは呼び出し側で 0 パディングする)。
    ///
    /// 内部の入出力バッファ (`input_scratch`/`output_scratch`) は毎回のアロケーションを
    /// 避けるため再利用する。戻り値のためのコピーは (呼び出し側に所有権を渡す必要があるため)
    /// 避けられないが、ラッパー用の `Vec<Vec<f32>>` を毎回新規確保していた分は無くなる。
    pub fn process(&mut self, mono_samples: &[f32]) -> Result<Vec<f32>, AudioError> {
        debug_assert_eq!(mono_samples.len(), self.input_frames);

        self.input_scratch[0].clear();
        self.input_scratch[0].extend_from_slice(mono_samples);
        let input_adapter = SequentialSliceOfVecs::new(&self.input_scratch, 1, mono_samples.len())
            .map_err(|e| AudioError::Resampler(format!("{e:?}")))?;

        let produced_frames = {
            let mut output_adapter =
                SequentialSliceOfVecs::new_mut(&mut self.output_scratch, 1, self.output_frames_max)
                    .map_err(|e| AudioError::Resampler(format!("{e:?}")))?;
            let (_, out_frames) = self
                .inner
                .process_into_buffer(&input_adapter, &mut output_adapter, None)
                .map_err(|e| AudioError::Resampler(e.to_string()))?;
            out_frames
        };

        Ok(self.output_scratch[0][..produced_frames].to_vec())
    }
}

/// マイク入力を開き、モノラル 16kHz PCM16LE の 100ms チャンクをコールバックへ渡す。
/// cpal の実デバイスが必要なため単体テストの対象外。
///
/// `device_name` が `None` なら OS既定のマイクを使う (design.md §5.8)。
pub fn open_input_stream(
    device_name: Option<&str>,
    mut on_chunk: impl FnMut(Vec<u8>) + Send + 'static,
) -> Result<cpal::Stream, AudioError> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

    let host = cpal::default_host();
    let device = match device_name {
        Some(name) => host
            .input_devices()
            .map_err(|e| AudioError::Device(e.to_string()))?
            .find(|d| d.description().map(|desc| desc.name() == name).unwrap_or(false))
            .ok_or_else(|| AudioError::Device(format!("マイク '{name}' が見つからない")))?,
        None => host
            .default_input_device()
            .ok_or_else(|| AudioError::Device("既定のマイクが見つからない".to_string()))?,
    };

    let config = device
        .default_input_config()
        .map_err(|e| AudioError::Device(e.to_string()))?;
    let input_rate = config.sample_rate();
    let channels = config.channels() as usize;
    // 100ms 分の入力フレーム数を単位としてリサンプラーへ渡す。
    let input_frames_per_call = (input_rate as usize / 1000) * (CHUNK_MS as usize);

    let mut resampler = MonoResampler::new(input_rate, input_frames_per_call)?;
    let mut mono_carry: Vec<f32> = Vec::with_capacity(input_frames_per_call * 2);
    let mut chunker = ChunkAccumulator::new();

    let error_callback = |err| tracing::error!("cpal 入力ストリームエラー: {err}");

    let stream = device
        .build_input_stream(
            config.config(),
            move |data: &[f32], _| {
                let mono = interleaved_to_mono(data, channels);
                mono_carry.extend_from_slice(&mono);
                while mono_carry.len() >= input_frames_per_call {
                    let frame: Vec<f32> = mono_carry.drain(..input_frames_per_call).collect();
                    match resampler.process(&frame) {
                        Ok(resampled) => {
                            let bytes = encode_pcm16le(&resampled);
                            for chunk in chunker.push(&bytes) {
                                on_chunk(chunk);
                            }
                        }
                        Err(e) => tracing::error!("リサンプル失敗: {e}"),
                    }
                }
            },
            error_callback,
            None,
        )
        .map_err(|e| AudioError::Stream(e.to_string()))?;

    stream.play().map_err(|e| AudioError::Stream(e.to_string()))?;
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downmix_averages_channels() {
        assert_eq!(downmix_to_mono(&[1.0, -1.0]), 0.0);
        assert_eq!(downmix_to_mono(&[0.5, 0.5, 0.5]), 0.5);
        assert_eq!(downmix_to_mono(&[]), 0.0);
    }

    #[test]
    fn interleaved_to_mono_downmixes_stereo() {
        // L, R, L, R
        let stereo = [1.0, -1.0, 0.5, 0.5];
        let mono = interleaved_to_mono(&stereo, 2);
        assert_eq!(mono, vec![0.0, 0.5]);
    }

    #[test]
    fn interleaved_to_mono_passthrough_for_mono_input() {
        let mono_in = [0.1, 0.2, 0.3];
        assert_eq!(interleaved_to_mono(&mono_in, 1), mono_in.to_vec());
    }

    #[test]
    fn pcm16_encoding_round_trips_within_rounding() {
        let samples = [0.0, 1.0, -1.0, 0.5];
        let bytes = encode_pcm16le(&samples);
        assert_eq!(bytes.len(), samples.len() * 2);
        let decoded: Vec<i16> = bytes
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect();
        assert_eq!(decoded[0], 0);
        assert_eq!(decoded[1], i16::MAX);
        assert_eq!(decoded[2], -i16::MAX);
    }

    #[test]
    fn pcm16_encoding_clips_out_of_range_values() {
        let bytes = encode_pcm16le(&[2.0, -2.0]);
        let decoded: Vec<i16> = bytes
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect();
        assert_eq!(decoded[0], i16::MAX);
        assert_eq!(decoded[1], -i16::MAX);
    }

    #[test]
    fn chunk_accumulator_yields_only_full_chunks_and_carries_remainder() {
        let mut acc = ChunkAccumulator::new();
        let half = vec![0u8; CHUNK_BYTES / 2];
        assert!(acc.push(&half).is_empty(), "半分では1チャンク出ないはず");
        let chunks = acc.push(&half);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].len(), CHUNK_BYTES);

        let leftover = vec![0u8; 10];
        assert!(acc.push(&leftover).is_empty());
        let flushed = acc.flush().expect("端数が残っているはず");
        assert_eq!(flushed.len(), 10);
        assert!(acc.flush().is_none(), "flush 後は空のはず");
    }

    #[test]
    fn pre_roll_buffer_keeps_only_last_300ms() {
        let mut buf = PreRollBuffer::new();
        let max = buf.max_bytes();
        buf.push(&vec![1u8; max]);
        buf.push(&[2u8; 100]);
        let snapshot = buf.snapshot();
        assert_eq!(snapshot.len(), max);
        assert!(snapshot.iter().rev().take(100).all(|&b| b == 2));
    }

    #[test]
    fn mono_resampler_48k_to_16k_shrinks_frame_count_by_roughly_a_third() {
        let input_frames = 4800; // 100ms @ 48kHz
        let mut resampler = MonoResampler::new(48_000, input_frames).expect("resampler 構築");
        let signal: Vec<f32> = (0..input_frames)
            .map(|i| (i as f32 * 0.05).sin())
            .collect();
        let output = resampler.process(&signal).expect("resample 実行");
        // 100ms @ 16kHz = 1600 フレーム前後 (FFT リサンプラーの遅延で厳密には一致しない)。
        assert!(
            (1400..=1800).contains(&output.len()),
            "output len={} が想定範囲外",
            output.len()
        );
    }

    /// レビュー指摘7 の再現テスト: `process` の内部入出力バッファをコンストラクタで1回だけ
    /// 確保して再利用する実装に変えた。この最適化が典型的に持ち込む事故
    /// (バッファを更新し忘れて毎回前回の出力を返してしまう・入力を正しく上書きできていない) が
    /// 無いことを確認する。
    ///
    /// 注記: FFT ベースのリサンプラーはブロック境界での連続性維持のため、多少のリンギング
    /// (前後ブロックの影響) が出力の端に残るのは resampler 自体の正常な挙動であり、
    /// これはバグではない。そのためここでは「無音入力の出力が無音に近いか」
    /// 「異なる入力に対して出力がきちんと変わるか」という、この最適化固有のリスクだけを見る。
    #[test]
    fn mono_resampler_reused_scratch_buffers_reflect_new_input_each_call() {
        let input_frames = 4800;
        let mut resampler = MonoResampler::new(48_000, input_frames).expect("resampler 構築");

        let silence = vec![0f32; input_frames];
        let tone: Vec<f32> = (0..input_frames).map(|i| (i as f32 * 0.05).sin()).collect();

        let out_silence = resampler.process(&silence).expect("無音の resample");
        let max_silence_amplitude = out_silence.iter().fold(0f32, |m, &s| m.max(s.abs()));
        assert!(
            max_silence_amplitude < 0.01,
            "無音入力 (1回目の呼び出し) の出力が無音になっていない: max={max_silence_amplitude}"
        );

        let out_tone = resampler.process(&tone).expect("トーンの resample");

        // 異なる入力を与えたのに出力がほぼ同じなら、スクラッチバッファが更新されずに
        // 前回の内容を返してしまっている疑いが強い (この最適化が典型的に持ち込む事故)。
        let compare_len = out_silence.len().min(out_tone.len());
        assert!(compare_len > 0);
        let max_diff = out_silence[..compare_len]
            .iter()
            .zip(out_tone[..compare_len].iter())
            .fold(0f32, |m, (a, b)| m.max((a - b).abs()));
        assert!(
            max_diff > 0.1,
            "無音とトーンで出力がほぼ同じ (スクラッチバッファが更新されていない疑い): max_diff={max_diff}"
        );
    }
}
